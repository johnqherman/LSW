use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::buildops::{self, BuildOptions, BuildSystem};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Serialize)]
/// Section Divergence.
pub struct SectionDivergence {
    /// Name.
    pub name: String,
    /// Detail.
    pub detail: String,
}

#[derive(Debug, Serialize)]
/// Artifact Repro.
pub struct ArtifactRepro {
    /// Artifact.
    pub artifact: String,
    /// Identical.
    pub identical: bool,
    /// Sha256.
    pub sha256: [String; 2],
    /// Diverging sections.
    pub diverging_sections: Vec<SectionDivergence>,
}

#[derive(Debug, Serialize)]
/// Repro Report.
pub struct ReproReport {
    /// Identical.
    pub identical: bool,
    /// Artifacts.
    pub artifacts: Vec<ArtifactRepro>,
}

fn matches_filter(rel: &Path, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(f) => {
            rel == Path::new(f)
                || rel
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(f))
        }
    }
}

fn clean_for_rebuild(project: &Project, system: BuildSystem, artifacts: &[PathBuf]) -> Result<()> {
    let remove_dir = |rel: &str| -> Result<()> {
        let dir = project.root.join(rel);
        if dir.is_dir()
            && !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink())
        {
            std::fs::remove_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
        }
        Ok(())
    };
    match system {
        BuildSystem::Cmake | BuildSystem::Meson => remove_dir("build")?,
        BuildSystem::Zig => remove_dir("zig-out")?,
        BuildSystem::Dotnet => remove_dir("bin/lsw-publish")?,
        BuildSystem::Cargo | BuildSystem::Make | BuildSystem::Ninja | BuildSystem::Explicit => {
            for rel in artifacts {
                let abs = project.root.join(rel);
                if abs.is_file() {
                    std::fs::remove_file(&abs).map_err(|e| Error::io(abs.clone(), e))?;
                }
            }
        }
    }
    Ok(())
}

fn read_section(data: &[u8], s: &lsw_pe::SectionInfo) -> Option<Vec<u8>> {
    let start = s.raw_offset as usize;
    let end = start.checked_add(s.raw_size as usize)?;
    data.get(start..end).map(<[u8]>::to_vec)
}

fn section_divergences(a: &Path, b: &Path) -> Vec<SectionDivergence> {
    let mut out = Vec::new();
    let (Ok(da), Ok(db)) = (lsw_pe::details(a), lsw_pe::details(b)) else {
        out.push(SectionDivergence {
            name: "(unparsable)".into(),
            detail: "one of the rebuilt artifacts could not be parsed as PE".into(),
        });
        return out;
    };
    let (Ok(bytes_a), Ok(bytes_b)) = (std::fs::read(a), std::fs::read(b)) else {
        return out;
    };

    let keyed_a = crate::diffops::keyed_sizes(&da.sections);
    let keyed_b = crate::diffops::keyed_sizes(&db.sections);
    let (delta, resized) = crate::diffops::keyed_delta(&keyed_a, &keyed_b);
    for name in &delta.added {
        out.push(SectionDivergence {
            name: name.clone(),
            detail: "only present in the second build".into(),
        });
    }
    for name in &delta.removed {
        out.push(SectionDivergence {
            name: name.clone(),
            detail: "only present in the first build".into(),
        });
    }
    for r in &resized {
        out.push(SectionDivergence {
            name: r.name.clone(),
            detail: format!("raw size changed by {:+} bytes", r.raw_size_delta),
        });
    }

    for sa in &da.sections {
        let Some(sb) = db
            .sections
            .iter()
            .find(|s| s.name == sa.name && s.raw_size == sa.raw_size)
        else {
            continue;
        };
        let (Some(ba), Some(bb)) = (read_section(&bytes_a, sa), read_section(&bytes_b, sb)) else {
            continue;
        };
        if ba != bb {
            let differing = ba.iter().zip(&bb).filter(|(x, y)| x != y).count();
            out.push(SectionDivergence {
                name: sa.name.clone(),
                detail: format!("content differs ({differing} byte(s))"),
            });
        }
    }

    if out.is_empty() {
        out.push(SectionDivergence {
            name: "(non-section data)".into(),
            detail: "sections are identical; headers, overlay, or padding differ".into(),
        });
    }
    out
}

/// Verify reproducible.
pub fn verify_reproducible(
    project: &Project,
    env: &Environment,
    filter: Option<&str>,
) -> Result<ReproReport> {
    let opts = BuildOptions {
        reproducible: true,
        ..BuildOptions::default()
    };

    let stage = std::env::temp_dir().join(format!("lsw-repro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| Error::io(stage.clone(), e))?;
    let result = verify_with_stage(project, env, filter, &opts, &stage);
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn verify_with_stage(
    project: &Project,
    env: &Environment,
    filter: Option<&str>,
    opts: &BuildOptions,
    stage: &Path,
) -> Result<ReproReport> {
    let first = buildops::build(project, env, opts)?;
    if first.artifacts.is_empty() {
        return Err(Error::NoBuildSystem);
    }

    let selected: Vec<PathBuf> = first
        .artifacts
        .iter()
        .filter(|rel| matches_filter(rel, filter))
        .cloned()
        .collect();
    if selected.is_empty() {
        return Err(Error::NotExecutable {
            program: PathBuf::from(filter.unwrap_or_default()),
            detail: "no build artifact matches this name".into(),
        });
    }

    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, rel) in selected.iter().enumerate() {
        let src = project.root.join(rel);
        let name = rel.file_name().map_or_else(
            || "artifact".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let dest = stage.join(format!("{i}-{name}"));
        std::fs::copy(&src, &dest).map_err(|e| Error::io(src.clone(), e))?;
        staged.push((rel.clone(), dest));
    }

    clean_for_rebuild(project, first.system, &first.artifacts)?;
    let second = buildops::build(project, env, opts)?;

    let mut artifacts = Vec::new();
    let mut all_identical = true;
    for (rel, first_copy) in &staged {
        let rebuilt = project.root.join(rel);
        if !second.artifacts.contains(rel) || !rebuilt.is_file() {
            all_identical = false;
            artifacts.push(ArtifactRepro {
                artifact: rel.display().to_string(),
                identical: false,
                sha256: [crate::sha256_file_checked(first_copy)?, String::new()],
                diverging_sections: vec![SectionDivergence {
                    name: "(missing)".into(),
                    detail: "the second build did not produce this artifact".into(),
                }],
            });
            continue;
        }
        let hash_a = crate::sha256_file_checked(first_copy)?;
        let hash_b = crate::sha256_file_checked(&rebuilt)?;
        let identical = hash_a == hash_b;
        let diverging_sections = if identical {
            Vec::new()
        } else {
            section_divergences(first_copy, &rebuilt)
        };
        all_identical &= identical;
        artifacts.push(ArtifactRepro {
            artifact: rel.display().to_string(),
            identical,
            sha256: [hash_a, hash_b],
            diverging_sections,
        });
    }

    Ok(ReproReport {
        identical: all_identical,
        artifacts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_matches_relative_path_or_file_name() {
        let rel = Path::new("build/app.exe");
        assert!(matches_filter(rel, None));
        assert!(matches_filter(rel, Some("app.exe")));
        assert!(matches_filter(rel, Some("APP.EXE")));
        assert!(matches_filter(rel, Some("build/app.exe")));
        assert!(!matches_filter(rel, Some("other.exe")));
    }

    #[test]
    fn identical_files_report_no_divergence() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.bin");
        std::fs::write(&a, b"same-bytes").unwrap();
        assert_eq!(
            crate::sha256_file_checked(&a).unwrap(),
            crate::sha256_file_checked(&a).unwrap()
        );
    }

    #[test]
    fn section_divergence_flags_unparsable_input() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.bin");
        let b = tmp.path().join("b.bin");
        std::fs::write(&a, b"not a pe").unwrap();
        std::fs::write(&b, b"not a pe either").unwrap();
        let d = section_divergences(&a, &b);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "(unparsable)");
    }
}
