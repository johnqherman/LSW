//! Wine version management: install, list, and remove local Wine builds.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

/// An installed Wine build managed by LSW.
#[derive(Debug, Serialize)]
pub struct WineInstallation {
    /// Version label (e.g. "9.0", "staging-9.0").
    pub version: String,
    /// Root directory of this Wine installation.
    pub path: PathBuf,
    /// Path to the wine executable inside this installation.
    pub executable: PathBuf,
}

/// List installed Wine versions in the LSW wine directory.
pub fn list(dirs: &lsw_config::Dirs) -> Vec<WineInstallation> {
    let wine_dir = dirs.wines();
    let Ok(entries) = fs::read_dir(&wine_dir) else {
        return Vec::new();
    };
    let mut out: Vec<WineInstallation> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if !path.is_dir() {
                return None;
            }
            let exe = find_wine_executable(&path)?;
            Some(WineInstallation {
                version: e.file_name().to_string_lossy().into_owned(),
                path,
                executable: exe,
            })
        })
        .collect();
    out.sort_by(|a, b| a.version.cmp(&b.version));
    out
}

/// Import a Wine build from a local directory into the managed wine store.
pub fn install(dirs: &lsw_config::Dirs, version: &str, source: &Path) -> Result<WineInstallation> {
    crate::envops::validate_name("wine version", version)?;
    let wine_dir = dirs.wines();
    let dest = wine_dir.join(version);
    if dest.is_dir() {
        if let Some(exe) = find_wine_executable(&dest) {
            return Ok(WineInstallation {
                version: version.to_owned(),
                path: dest,
                executable: exe,
            });
        }
        fs::remove_dir_all(&dest).map_err(|e| Error::io(dest.clone(), e))?;
    }

    if !source.exists() {
        return Err(Error::io(
            source.to_path_buf(),
            std::io::Error::new(std::io::ErrorKind::NotFound, "source path does not exist"),
        ));
    }

    fs::create_dir_all(&dest).map_err(|e| Error::io(dest.clone(), e))?;

    if source.is_dir() {
        copy_tree(source, &dest)?;
    } else {
        extract_tarball(source, &dest)?;
    }

    let executable = find_wine_executable(&dest).ok_or_else(|| Error::ToolMissing {
        tool: "wine".into(),
        fix: format!(
            "the imported directory does not contain bin/wine or bin/wine64; check {}",
            source.display()
        ),
    })?;

    Ok(WineInstallation {
        version: version.to_owned(),
        path: dest,
        executable,
    })
}

/// Remove an installed Wine version.
pub fn remove(dirs: &lsw_config::Dirs, version: &str) -> Result<bool> {
    crate::envops::validate_name("wine version", version)?;
    let path = dirs.wines().join(version);
    if !path.is_dir() {
        return Ok(false);
    }
    fs::remove_dir_all(&path).map_err(|e| Error::io(path, e))?;
    Ok(true)
}

/// Resolve a version label to the wine executable path.
pub fn resolve(dirs: &lsw_config::Dirs, version: &str) -> Result<PathBuf> {
    crate::envops::validate_name("wine version", version)?;
    let path = dirs.wines().join(version);
    find_wine_executable(&path).ok_or_else(|| Error::ToolMissing {
        tool: format!("wine (version {version})"),
        fix: format!("install it with: lsw wine install {version} --from <path>"),
    })
}

fn find_wine_executable(root: &Path) -> Option<PathBuf> {
    let wine64 = root.join("bin/wine64");
    if wine64.is_file() {
        return Some(wine64);
    }
    let wine = root.join("bin/wine");
    if wine.is_file() {
        return Some(wine);
    }
    None
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    const MAX_ENTRIES: usize = 500_000;
    const MAX_DEPTH: usize = 64;
    copy_tree_depth(src, dst, MAX_DEPTH, &mut 0, MAX_ENTRIES)
}

fn copy_tree_depth(
    src: &Path,
    dst: &Path,
    depth: usize,
    count: &mut usize,
    max: usize,
) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|e| Error::io(dst.to_path_buf(), e))?;
    let entries = fs::read_dir(src).map_err(|e| Error::io(src.to_path_buf(), e))?;
    for entry in entries.flatten() {
        *count += 1;
        if *count > max {
            return Ok(());
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let name = entry.file_name();
        let target = dst.join(&name);
        if meta.is_dir() {
            copy_tree_depth(&entry.path(), &target, depth - 1, count, max)?;
        } else if meta.is_file() {
            fs::copy(entry.path(), &target).map_err(|e| Error::io(target, e))?;
        }
    }
    Ok(())
}

fn extract_tarball(archive: &Path, dest: &Path) -> Result<()> {
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("--strip-components=1")
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|e| Error::io(archive.to_path_buf(), e))?;
    if !status.success() {
        return Err(Error::ExtractFailed {
            name: archive.display().to_string(),
            detail: format!("tar exited with {status}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dirs(tmp: &tempfile::TempDir) -> lsw_config::Dirs {
        lsw_config::Dirs {
            data: tmp.path().join("data"),
            config: tmp.path().join("config"),
            cache: tmp.path().join("cache"),
        }
    }

    #[test]
    fn list_empty_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        assert!(list(&dirs).is_empty());
    }

    #[test]
    fn list_finds_installed_wine() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        let wine_dir = dirs.wines().join("9.0").join("bin");
        fs::create_dir_all(&wine_dir).unwrap();
        fs::write(wine_dir.join("wine64"), b"#!/bin/sh\n").unwrap();
        let found = list(&dirs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version, "9.0");
        assert!(found[0].executable.ends_with("bin/wine64"));
    }

    #[test]
    fn list_skips_dirs_without_wine_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        fs::create_dir_all(dirs.wines().join("broken")).unwrap();
        assert!(list(&dirs).is_empty());
    }

    #[test]
    fn resolve_missing_version_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        assert!(resolve(&dirs, "nonexistent").is_err());
    }

    #[test]
    fn resolve_finds_wine64() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        let bin = dirs.wines().join("8.0").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("wine64"), b"#!/bin/sh\n").unwrap();
        let exe = resolve(&dirs, "8.0").unwrap();
        assert!(exe.ends_with("bin/wine64"));
    }

    #[test]
    fn resolve_falls_back_to_wine() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        let bin = dirs.wines().join("7.0").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("wine"), b"#!/bin/sh\n").unwrap();
        let exe = resolve(&dirs, "7.0").unwrap();
        assert!(exe.ends_with("bin/wine"));
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        assert!(!remove(&dirs, "ghost").unwrap());
    }

    #[test]
    fn remove_deletes_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        let bin = dirs.wines().join("9.0").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("wine64"), b"#!/bin/sh\n").unwrap();
        assert!(remove(&dirs, "9.0").unwrap());
        assert!(!dirs.wines().join("9.0").exists());
    }

    #[test]
    fn install_from_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        let src = tmp.path().join("wine-source");
        let src_bin = src.join("bin");
        fs::create_dir_all(&src_bin).unwrap();
        fs::write(src_bin.join("wine64"), b"#!/bin/sh\n").unwrap();
        fs::write(src_bin.join("wineserver"), b"#!/bin/sh\n").unwrap();

        let result = install(&dirs, "test-1.0", &src).unwrap();
        assert_eq!(result.version, "test-1.0");
        assert!(result.executable.ends_with("bin/wine64"));
        assert!(dirs.wines().join("test-1.0/bin/wine64").is_file());
        assert!(dirs.wines().join("test-1.0/bin/wineserver").is_file());
    }

    #[test]
    fn install_rejects_invalid_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        assert!(install(&dirs, "../escape", Path::new("/tmp")).is_err());
    }

    #[test]
    fn install_missing_source_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(&tmp);
        assert!(install(&dirs, "1.0", Path::new("/nonexistent/wine")).is_err());
    }
}
