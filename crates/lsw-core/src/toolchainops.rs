use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};
use lsw_config::Dirs;

const LLVM_MINGW_REPO: &str = "https://github.com/mstorsjo/llvm-mingw";

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub name: String,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct InstalledToolchain {
    pub name: String,
    pub path: String,
}

pub fn list(dirs: &Dirs) -> Vec<InstalledToolchain> {
    let Ok(entries) = std::fs::read_dir(dirs.toolchains()) else {
        return Vec::new();
    };
    let mut out: Vec<InstalledToolchain> = entries
        .flatten()
        .filter(|e| e.path().join("bin").is_dir())
        .map(|e| InstalledToolchain {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().display().to_string(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn remove(dirs: &Dirs, name: &str) -> Result<bool> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Ok(false);
    }
    let path = dirs.toolchains().join(name);
    if !path.is_dir() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&path).map_err(|e| Error::io(path, e))?;
    Ok(true)
}

pub fn install(dirs: &Dirs, spec: &str) -> Result<InstallReport> {
    let (name, version) = match spec.split_once('@') {
        Some((n, v)) => (n, v.to_owned()),
        None => (spec, resolve_latest_tag()?),
    };
    if name != "llvm-mingw" {
        return Err(Error::ToolMissing {
            tool: name.to_owned(),
            fix:
                "only llvm-mingw is installable today; use LSW_TOOLCHAIN_DIRS for other toolchains"
                    .into(),
        });
    }
    let host_arch = std::env::consts::ARCH;
    let dest = dirs.toolchains().join(format!("llvm-mingw-{version}"));
    if dest.join("bin").is_dir() {
        return Ok(InstallReport {
            name: name.to_owned(),
            version,
            path: dest.display().to_string(),
        });
    }

    let mut last_err = None;
    for distro in ["ubuntu-22.04", "ubuntu-20.04", "ubuntu-18.04"] {
        let asset = format!("llvm-mingw-{version}-ucrt-{distro}-{host_arch}.tar.xz");
        let url = format!("{LLVM_MINGW_REPO}/releases/download/{version}/{asset}");
        let cached = dirs.cache.join("toolchains").join(&asset);
        let downloaded = cached.is_file()
            || match curl_download(&url, &cached) {
                Ok(()) => true,
                Err(e) => {
                    last_err = Some(e);
                    false
                }
            };
        if !downloaded {
            continue;
        }
        std::fs::create_dir_all(&dest).map_err(|e| Error::io(dest.clone(), e))?;
        let status = Command::new("tar")
            .arg("-xJf")
            .arg(&cached)
            .arg("--strip-components=1")
            .arg("-C")
            .arg(&dest)
            .status()
            .map_err(|e| Error::io(PathBuf::from("tar"), e))?;
        if !status.success() {
            let _ = std::fs::remove_dir_all(&dest);
            let _ = std::fs::remove_file(&cached);
            return Err(Error::ExtractFailed {
                name: asset,
                detail: "extracting the toolchain archive failed".into(),
            });
        }
        return Ok(InstallReport {
            name: name.to_owned(),
            version,
            path: dest.display().to_string(),
        });
    }
    Err(last_err.unwrap_or_else(|| Error::DownloadFailed {
        url: format!("{LLVM_MINGW_REPO}/releases/download/{version}/"),
        detail: format!("no llvm-mingw {version} release asset for {host_arch}"),
    }))
}

fn resolve_latest_tag() -> Result<String> {
    let url = format!("{LLVM_MINGW_REPO}/releases/latest");
    let out = Command::new("curl")
        .args(["-fsSLI", "-o", "/dev/null", "-w", "%{url_effective}"])
        .arg(&url)
        .output()
        .map_err(|e| Error::io(PathBuf::from("curl"), e))?;
    if !out.status.success() {
        return Err(Error::DownloadFailed {
            url,
            detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    let effective = String::from_utf8_lossy(&out.stdout);
    effective
        .trim()
        .rsplit('/')
        .next()
        .filter(|tag| !tag.is_empty() && *tag != "latest")
        .map(str::to_owned)
        .ok_or_else(|| Error::DownloadFailed {
            url,
            detail: "could not resolve the latest release tag; pass llvm-mingw@<version>".into(),
        })
}

fn curl_download(url: &str, dest: &std::path::Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
    }
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "2",
            "--max-filesize",
            "1073741824",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .output()
        .map_err(|e| Error::io(PathBuf::from("curl"), e))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(dest);
        return Err(Error::DownloadFailed {
            url: url.to_owned(),
            detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dirs(base: &std::path::Path) -> Dirs {
        Dirs {
            data: base.join("share"),
            config: base.join("config"),
            cache: base.join("cache"),
        }
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        assert!(list(&dirs).is_empty());
    }

    #[test]
    fn list_finds_toolchains_with_bin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        let tc_dir = dirs.toolchains().join("llvm-mingw-20241231");
        std::fs::create_dir_all(tc_dir.join("bin")).unwrap();
        let found = list(&dirs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "llvm-mingw-20241231");
    }

    #[test]
    fn list_skips_entries_without_bin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        let tc_dir = dirs.toolchains().join("incomplete-toolchain");
        std::fs::create_dir_all(&tc_dir).unwrap();
        assert!(list(&dirs).is_empty());
    }

    #[test]
    fn list_sorted_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        for name in ["zzz-tc", "aaa-tc", "mmm-tc"] {
            std::fs::create_dir_all(dirs.toolchains().join(name).join("bin")).unwrap();
        }
        let found = list(&dirs);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].name, "aaa-tc");
        assert_eq!(found[1].name, "mmm-tc");
        assert_eq!(found[2].name, "zzz-tc");
    }

    #[test]
    fn remove_returns_false_for_nonexistent() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        std::fs::create_dir_all(dirs.toolchains()).unwrap();
        assert!(!remove(&dirs, "nope").unwrap());
    }

    #[test]
    fn remove_rejects_path_traversal_slash() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        assert!(!remove(&dirs, "../../etc").unwrap());
    }

    #[test]
    fn remove_rejects_path_traversal_backslash() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        assert!(!remove(&dirs, "foo\\bar").unwrap());
    }

    #[test]
    fn remove_rejects_dotdot() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        assert!(!remove(&dirs, "..").unwrap());
    }

    #[test]
    fn remove_deletes_existing_toolchain() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        let tc_dir = dirs.toolchains().join("my-tc");
        std::fs::create_dir_all(tc_dir.join("bin")).unwrap();
        assert!(remove(&dirs, "my-tc").unwrap());
        assert!(!tc_dir.exists());
    }

    #[test]
    fn install_rejects_unknown_toolchain() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        let result = install(&dirs, "unknown-tc@1.0");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::ToolMissing { .. }));
    }

    #[test]
    fn install_returns_cached_if_already_present() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = test_dirs(tmp.path());
        let tc_dir = dirs.toolchains().join("llvm-mingw-20241231");
        std::fs::create_dir_all(tc_dir.join("bin")).unwrap();
        let report = install(&dirs, "llvm-mingw@20241231").unwrap();
        assert_eq!(report.name, "llvm-mingw");
        assert_eq!(report.version, "20241231");
    }
}
