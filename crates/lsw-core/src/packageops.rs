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
    Msi,
}

#[derive(Debug, Serialize)]
pub struct PackageReport {
    pub directory: PathBuf,
    pub zip: Option<PathBuf>,
    pub msi: Option<PathBuf>,
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

    let mut seen: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    for artifact in &build.artifacts {
        let name = artifact
            .file_name()
            .expect("artifacts always have file names")
            .to_string_lossy()
            .into_owned();
        if let Some(previous) = seen.insert(name.clone(), artifact.clone()) {
            return Err(Error::PackageNameCollision {
                name,
                first: previous,
                second: artifact.clone(),
            });
        }
    }

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

    let mut zip = None;
    let mut msi = None;
    match target {
        PackageTarget::PortableDirectory => {}
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
            zip = Some(zip_path);
        }
        PackageTarget::Msi => {
            msi = Some(build_msi(project, env, &dist, &dir, &stem, &files)?);
        }
    }

    Ok(PackageReport {
        directory: dir,
        zip,
        msi,
        files,
    })
}

fn build_msi(
    project: &Project,
    env: &Environment,
    dist: &std::path::Path,
    dir: &std::path::Path,
    stem: &str,
    files: &[String],
) -> Result<PathBuf> {
    if which("wixl").is_none() {
        return Err(Error::ToolMissing {
            tool: "wixl".into(),
            fix: "install msitools (provides wixl), or use --target zip".into(),
        });
    }

    let name = &project.manifest.project.name;
    let wxs = render_wxs(name, files);
    let wxs_path = dist.join(format!("{stem}.wxs"));
    fs::write(&wxs_path, wxs).map_err(|e| Error::io(wxs_path.clone(), e))?;

    let msi_path = dist.join(format!("{stem}.msi"));
    if msi_path.exists() {
        fs::remove_file(&msi_path).map_err(|e| Error::io(msi_path.clone(), e))?;
    }

    let arch = match env.manifest.target_arch {
        lsw_config::TargetArch::X86_64 => "x64",
        lsw_config::TargetArch::X86 => "x86",
        lsw_config::TargetArch::Aarch64 | lsw_config::TargetArch::Arm64Ec => "arm64",
        lsw_config::TargetArch::Armv7 => "arm",
    };

    let abs_wxs = std::path::absolute(&wxs_path).map_err(|e| Error::io(wxs_path.clone(), e))?;
    let abs_msi = std::path::absolute(&msi_path).map_err(|e| Error::io(msi_path.clone(), e))?;
    let output = Command::new("wixl")
        .arg("-a")
        .arg(arch)
        .arg("-o")
        .arg(&abs_msi)
        .arg(&abs_wxs)
        .current_dir(dir)
        .output()
        .map_err(|e| Error::io(PathBuf::from("wixl"), e))?;
    if !output.status.success() {
        return Err(Error::BuildFailed {
            command: format!(
                "wixl -a {arch} -o {} {}: {}",
                msi_path.display(),
                wxs_path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            code: output.status.code(),
        });
    }
    Ok(msi_path)
}

fn render_wxs(name: &str, files: &[String]) -> String {
    let upgrade_code = deterministic_guid(&format!("lsw:{name}:upgrade"));

    let mut components = String::new();
    let mut refs = String::new();
    for (i, file) in files.iter().enumerate() {
        let comp_id = format!("cmp{i}");
        let file_id = format!("file{i}");
        let guid = deterministic_guid(&format!("lsw:{name}:{file}"));
        components.push_str(&format!(
            "          <Component Id=\"{comp_id}\" Guid=\"{guid}\">\n\
             \x20           <File Id=\"{file_id}\" Source=\"{file}\" KeyPath=\"yes\"/>\n\
             \x20         </Component>\n"
        ));
        refs.push_str(&format!("        <ComponentRef Id=\"{comp_id}\"/>\n"));
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <Wix xmlns=\"http://schemas.microsoft.com/wix/2006/wi\">\n\
         \x20 <Product Id=\"*\" Name=\"{name}\" Language=\"1033\" Version=\"1.0.0\"\n\
         \x20          Manufacturer=\"LSW\" UpgradeCode=\"{upgrade_code}\">\n\
         \x20   <Package InstallerVersion=\"200\" Compressed=\"yes\" InstallScope=\"perMachine\"/>\n\
         \x20   <Media Id=\"1\" Cabinet=\"main.cab\" EmbedCab=\"yes\"/>\n\
         \x20   <Directory Id=\"TARGETDIR\" Name=\"SourceDir\">\n\
         \x20     <Directory Id=\"ProgramFilesFolder\">\n\
         \x20       <Directory Id=\"INSTALLDIR\" Name=\"{name}\">\n\
         {components}\
         \x20       </Directory>\n\
         \x20     </Directory>\n\
         \x20   </Directory>\n\
         \x20   <Feature Id=\"Main\" Title=\"{name}\" Level=\"1\">\n\
         {refs}\
         \x20   </Feature>\n\
         \x20 </Product>\n\
         </Wix>\n"
    )
}

fn deterministic_guid(seed: &str) -> String {
    let hex = lsw_toolchain::sha256_bytes(seed.as_bytes());
    let b = hex.as_bytes();
    let s = |start: usize, len: usize| -> String {
        std::str::from_utf8(&b[start..start + len])
            .unwrap()
            .to_ascii_uppercase()
    };
    format!(
        "{}-{}-{}-{}-{}",
        s(0, 8),
        s(8, 4),
        s(12, 4),
        s(16, 4),
        s(20, 12)
    )
}
