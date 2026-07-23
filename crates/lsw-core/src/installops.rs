use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::envops::{self, Environment};
use crate::error::{Error, Result};
use lsw_config::Dirs;

#[derive(Debug, Serialize)]
pub struct InstallVerifyReport {
    pub msi: PathBuf,
    pub install_dir: PathBuf,
    pub files: Vec<String>,
    pub uninstall_clean: bool,
}

pub fn verify_msi(
    dirs: &Dirs,
    env: &Environment,
    name: &str,
    msi: &Path,
    files: &[String],
) -> Result<InstallVerifyReport> {
    let scratch_name = format!("{}-pkgverify", env.manifest.name);
    let scratch = envops::clone_env(dirs, &env.manifest.name, &scratch_name, true)?;
    let result = run_verify(&scratch, name, msi, files);
    let _ = envops::remove(dirs, &scratch_name);
    result
}

fn run_verify(
    scratch: &Environment,
    name: &str,
    msi: &Path,
    files: &[String],
) -> Result<InstallVerifyReport> {
    let msi = std::path::absolute(msi).map_err(|e| Error::io(msi.to_path_buf(), e))?;
    let msi_win = windows_msi_path(&msi);
    let prefix = scratch.layout.prefix();

    let install = msiexec(scratch, &prefix, &["/i", &msi_win, "/qn"])?;
    if !install.status.success() {
        return Err(Error::InstallVerifyFailed {
            stage: "install".into(),
            detail: msiexec_failure(&install),
        });
    }

    let install_dir = match find_install_dir(&prefix, name, files) {
        Some(dir) => dir,
        None => {
            let candidates = install_dir_candidates(&prefix, name);
            let _ = msiexec(scratch, &prefix, &["/x", &msi_win, "/qn"]);
            return Err(Error::InstallVerifyFailed {
                stage: "install".into(),
                detail: format!(
                    "installed files not found under {}",
                    candidates
                        .iter()
                        .map(|c| c.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" or ")
                ),
            });
        }
    };

    let uninstall = msiexec(scratch, &prefix, &["/x", &msi_win, "/qn"])?;
    if !uninstall.status.success() {
        return Err(Error::InstallVerifyFailed {
            stage: "uninstall".into(),
            detail: msiexec_failure(&uninstall),
        });
    }

    let leftovers: Vec<String> = files
        .iter()
        .filter(|f| install_dir.join(f).exists())
        .cloned()
        .collect();
    if !leftovers.is_empty() {
        return Err(Error::InstallVerifyFailed {
            stage: "uninstall".into(),
            detail: format!(
                "files left behind after uninstall: {}",
                leftovers.join(", ")
            ),
        });
    }

    Ok(InstallVerifyReport {
        msi,
        install_dir,
        files: files.to_vec(),
        uninstall_clean: true,
    })
}

fn msiexec(scratch: &Environment, prefix: &Path, args: &[&str]) -> Result<std::process::Output> {
    let wine = &scratch.manifest.runtime.executable;
    let mut command = Command::new(wine);
    command.arg("msiexec").args(args);
    lsw_runtime::scrub_wine_env(&mut command);
    for (k, v) in lsw_runtime::base_env(prefix) {
        command.env(k, v);
    }
    command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing {
                tool: wine.display().to_string(),
                fix: "install Wine or recreate the environment with lsw env create".into(),
            }
        } else {
            Error::io(wine.clone(), e)
        }
    })
}

fn msiexec_failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    match output.status.code() {
        Some(code) if trimmed.is_empty() => format!("msiexec exited with code {code}"),
        Some(code) => format!("msiexec exited with code {code}: {trimmed}"),
        None => format!("msiexec terminated by signal: {trimmed}"),
    }
}

fn install_dir_candidates(prefix: &Path, name: &str) -> Vec<PathBuf> {
    let drive_c = prefix.join("drive_c");
    vec![
        drive_c.join("Program Files").join(name),
        drive_c.join("Program Files (x86)").join(name),
    ]
}

fn find_install_dir(prefix: &Path, name: &str, files: &[String]) -> Option<PathBuf> {
    install_dir_candidates(prefix, name)
        .into_iter()
        .find(|dir| dir.is_dir() && files.iter().all(|f| dir.join(f).is_file()))
}

fn windows_msi_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_msi_path_maps_to_z_drive() {
        assert_eq!(
            windows_msi_path(Path::new("/home/u/dist/app.msi")),
            "Z:\\home\\u\\dist\\app.msi"
        );
    }

    #[test]
    fn find_install_dir_requires_all_files() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path();
        let dir = prefix.join("drive_c").join("Program Files").join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.exe"), b"x").unwrap();

        let one = vec!["app.exe".to_owned()];
        assert_eq!(find_install_dir(prefix, "app", &one), Some(dir));

        let two = vec!["app.exe".to_owned(), "helper.dll".to_owned()];
        assert_eq!(find_install_dir(prefix, "app", &two), None);
    }

    #[test]
    fn install_dir_candidates_cover_both_program_files() {
        let candidates = install_dir_candidates(Path::new("/p"), "app");
        assert_eq!(candidates.len(), 2);
        assert!(candidates[0].ends_with("Program Files/app"));
        assert!(candidates[1].ends_with("Program Files (x86)/app"));
    }
}
