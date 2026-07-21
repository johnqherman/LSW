use std::fs;
use std::path::PathBuf;
use std::process::Command;

use lsw_config::Lockfile;

use crate::envops::{self, Environment};
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildSystem {
    Cmake,
    Cargo,
    Explicit,
}

#[derive(Debug)]
pub struct BuildOptions {
    pub system: Option<String>,
    pub update_lock: bool,
}

#[derive(Debug)]
pub struct BuildReport {
    pub system: BuildSystem,
    pub commands: Vec<String>,
    pub artifacts: Vec<PathBuf>,
    pub lock_written: bool,
}

pub fn build(project: &Project, env: &Environment, opts: &BuildOptions) -> Result<BuildReport> {
    envops::link_project(env, project)?;
    let lock_written = sync_lockfile(project, env, opts.update_lock)?;
    stamp_build_dir(project, env)?;

    let explicit = project.manifest.build.as_ref();
    let system = match (opts.system.as_deref(), explicit) {
        (Some("cmake"), _) => BuildSystem::Cmake,
        (Some("cargo"), _) => BuildSystem::Cargo,
        (Some(_), Some(_)) | (None, Some(_)) => BuildSystem::Explicit,
        (Some(_), None) | (None, None) => {
            if project.root.join("CMakeLists.txt").is_file() {
                BuildSystem::Cmake
            } else if project.root.join("Cargo.toml").is_file() {
                BuildSystem::Cargo
            } else {
                return Err(Error::NoBuildSystem);
            }
        }
    };

    let mut commands = Vec::new();
    let mut artifact_dir = project.root.join("build");
    match system {
        BuildSystem::Explicit => {
            let spec = explicit.ok_or(Error::NoBuildSystem)?;
            run_step(project, env, &spec.command, &mut commands)?;
        }
        BuildSystem::Cargo => {
            let triple = env.manifest.target_arch.rust_gnu_triple().ok_or_else(|| {
                Error::RustTargetUnavailable {
                    arch: env.manifest.target_arch.to_string(),
                }
            })?;
            crate::rustops::ensure_target(env.manifest.target_arch)?;
            run_step(
                project,
                env,
                &[
                    "cargo".to_owned(),
                    "build".to_owned(),
                    "--target".to_owned(),
                    triple.to_owned(),
                ],
                &mut commands,
            )?;
            artifact_dir = project.root.join("target").join(triple).join("debug");
        }
        BuildSystem::Cmake => {
            let toolchain_file = env.layout.cmake_toolchain_file();
            lsw_toolchain::write_cmake_toolchain_file(
                &toolchain_file,
                &env.manifest.toolchain,
                env.manifest.target_arch,
            )
            .map_err(|e| Error::io(toolchain_file.clone(), e))?;

            let generator = if which("ninja").is_some() {
                Some("Ninja")
            } else {
                None
            };
            let mut configure = vec![
                "cmake".to_owned(),
                "-S".to_owned(),
                ".".to_owned(),
                "-B".to_owned(),
                "build".to_owned(),
                format!("-DCMAKE_TOOLCHAIN_FILE={}", toolchain_file.display()),
                "-DCMAKE_BUILD_TYPE=Debug".to_owned(),
                format!(
                    "-DCMAKE_CROSSCOMPILING_EMULATOR={}",
                    env.manifest.runtime.executable.display()
                ),
            ];
            if let Some(g) = generator {
                configure.push("-G".to_owned());
                configure.push(g.to_owned());
            }
            run_step(project, env, &configure, &mut commands)?;
            run_step(
                project,
                env,
                &["cmake".to_owned(), "--build".to_owned(), "build".to_owned()],
                &mut commands,
            )?;
        }
    }

    let artifacts = find_artifacts(&artifact_dir, &project.root);
    verify_artifacts_are_pe(project, &artifacts)?;

    Ok(BuildReport {
        system,
        commands,
        artifacts,
        lock_written,
    })
}

fn stamp_build_dir(project: &Project, env: &Environment) -> Result<()> {
    let build_dir = project.root.join("build");
    let marker = build_dir.join(".lsw-env");
    if build_dir.is_dir() {
        let owner = fs::read_to_string(&marker).unwrap_or_default();
        if owner.trim() != env.name {
            fs::remove_dir_all(&build_dir).map_err(|e| Error::io(build_dir.clone(), e))?;
        }
    }
    fs::create_dir_all(&build_dir).map_err(|e| Error::io(build_dir.clone(), e))?;
    fs::write(&marker, &env.name).map_err(|e| Error::io(marker, e))
}

fn verify_artifacts_are_pe(project: &Project, artifacts: &[PathBuf]) -> Result<()> {
    for artifact in artifacts {
        let absolute = project.root.join(artifact);
        match lsw_pe::detect(&absolute)? {
            lsw_pe::BinaryKind::Pe(_) => {}
            other => {
                return Err(Error::ArtifactNotPe {
                    artifact: artifact.clone(),
                    found: format!("{other:?}"),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn check_lock(project: &Project, env: &Environment) -> Result<()> {
    let path = project.lockfile_path();
    if !path.is_file() {
        return Ok(());
    }
    let recorded = Lockfile::load(&path)?;
    let current = envops::lockfile_for(env)?;
    if recorded != current {
        return Err(Error::LockMismatch {
            environment: env.name.clone(),
            detail: lock_diff(&recorded, &current),
        });
    }
    Ok(())
}

fn run_step(
    project: &Project,
    env: &Environment,
    argv: &[String],
    commands: &mut Vec<String>,
) -> Result<()> {
    let (program, args) = argv.split_first().ok_or(Error::NoBuildSystem)?;
    let rendered = argv.join(" ");
    commands.push(rendered.clone());

    let tc = &env.manifest.toolchain;
    let c_flags = tc.c_flags.join(" ");
    let cxx_flags = tc
        .c_flags
        .iter()
        .chain(&tc.cxx_flags)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let link_flags = tc.link_flags.join(" ");
    let mut command = Command::new(program);
    lsw_runtime::scrub_wine_env(&mut command);
    command
        .args(args)
        .current_dir(&project.root)
        .env("WINEPREFIX", env.layout.prefix())
        .env("CC", &tc.cc)
        .env("CXX", &tc.cxx)
        .env("CFLAGS", &c_flags)
        .env("CXXFLAGS", &cxx_flags)
        .env("LDFLAGS", &link_flags)
        .env("LSW_ENV", &env.name)
        .env("LSW_TARGET_FLAGS", &c_flags);
    let status = command.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing {
                tool: program.clone(),
                fix: format!("install {program} or adjust [build].command in lsw.toml"),
            }
        } else {
            Error::io(PathBuf::from(program), e)
        }
    })?;

    if !status.success() {
        return Err(Error::BuildFailed {
            command: rendered,
            code: status.code(),
        });
    }
    Ok(())
}

fn sync_lockfile(project: &Project, env: &Environment, update: bool) -> Result<bool> {
    let current = envops::lockfile_for(env)?;
    let path = project.lockfile_path();
    if !path.is_file() || update {
        current.save(&path)?;
        return Ok(true);
    }
    let recorded = Lockfile::load(&path)?;
    if recorded != current {
        return Err(Error::LockMismatch {
            environment: env.name.clone(),
            detail: lock_diff(&recorded, &current),
        });
    }
    Ok(false)
}

fn lock_diff(recorded: &Lockfile, current: &Lockfile) -> String {
    let mut lines = Vec::new();
    let pairs = [
        ("toolchain", &recorded.toolchain, &current.toolchain),
        ("runtime", &recorded.runtime, &current.runtime),
        ("sysroot", &recorded.sysroot, &current.sysroot),
    ];
    for (label, rec, cur) in pairs {
        if rec != cur {
            lines.push(format!(
                "  {label}: locked {} {} ({}...) but environment has {} {} ({}...)",
                rec.provider,
                rec.version,
                &rec.sha256[..12.min(rec.sha256.len())],
                cur.provider,
                cur.version,
                &cur.sha256[..12.min(cur.sha256.len())],
            ));
        }
    }
    if recorded.target_arch != current.target_arch {
        lines.push(format!(
            "  arch: locked {} but environment targets {}",
            recorded.target_arch, current.target_arch
        ));
    }
    lines.join("\n")
}

fn find_artifacts(build_dir: &std::path::Path, project_root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(build_dir, &mut out);
    out.sort();
    out.into_iter()
        .map(|p| p.strip_prefix(project_root).map(PathBuf::from).unwrap_or(p))
        .collect()
}

fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "CMakeFiles") {
                continue;
            }
            walk(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
        {
            out.push(path);
        }
    }
}

pub(crate) fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(program))
        .find(|c| c.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_sh() {
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-real-tool-xyz").is_none());
    }

    #[test]
    fn artifacts_walk_skips_cmakefiles() {
        let tmp = tempfile::tempdir().unwrap();
        let build = tmp.path().join("build");
        fs::create_dir_all(build.join("CMakeFiles/x")).unwrap();
        fs::write(build.join("app.exe"), b"MZ").unwrap();
        fs::write(build.join("CMakeFiles/x/probe.exe"), b"MZ").unwrap();
        fs::write(build.join("notes.txt"), b"x").unwrap();
        let found = find_artifacts(&build, tmp.path());
        assert_eq!(found, vec![PathBuf::from("build/app.exe")]);
    }
}
