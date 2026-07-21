use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::buildops::{self, BuildOptions, which};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageTarget {
    PortableDirectory,
    Zip,
}

#[derive(Debug, Serialize)]
pub struct PackageReport {
    pub directory: PathBuf,
    pub zip: Option<PathBuf>,
    pub files: Vec<String>,
}

pub fn package(
    project: &Project,
    env: &Environment,
    target: PackageTarget,
) -> Result<PackageReport> {
    let build = buildops::build(
        project,
        env,
        &BuildOptions {
            system: None,
            update_lock: false,
        },
    )?;
    if build.artifacts.is_empty() {
        return Err(Error::NoBuildSystem);
    }

    let stem = format!(
        "{}-{}",
        project.manifest.project.name, env.manifest.target_arch
    );
    let dist = project.root.join("dist");
    let dir = dist.join(&stem);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
    }
    fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;

    let mut files = Vec::new();
    for artifact in &build.artifacts {
        let source = project.root.join(artifact);
        let name = source
            .file_name()
            .expect("artifacts always have file names")
            .to_owned();
        let dest = dir.join(&name);
        fs::copy(&source, &dest).map_err(|e| Error::io(source.clone(), e))?;
        files.push(name.to_string_lossy().into_owned());
    }

    let zip = match target {
        PackageTarget::PortableDirectory => None,
        PackageTarget::Zip => {
            if which("zip").is_none() {
                return Err(Error::ToolMissing {
                    tool: "zip".into(),
                    fix: "install zip, or use --target portable-directory".into(),
                });
            }
            let zip_path = dist.join(format!("{stem}.zip"));
            if zip_path.exists() {
                fs::remove_file(&zip_path).map_err(|e| Error::io(zip_path.clone(), e))?;
            }
            let status = Command::new("zip")
                .args(["-r", "-q"])
                .arg(&zip_path)
                .arg(&stem)
                .current_dir(&dist)
                .status()
                .map_err(|e| Error::io(zip_path.clone(), e))?;
            if !status.success() {
                return Err(Error::BuildFailed {
                    command: format!("zip -r {} {stem}", zip_path.display()),
                    code: status.code(),
                });
            }
            Some(zip_path)
        }
    };

    Ok(PackageReport {
        directory: dir,
        zip,
        files,
    })
}
