use std::fs;

use serde::Serialize;

use lsw_config::TargetArch;

use crate::envops::Environment;
use crate::error::{Error, Result};

const TEMPLATE_CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net8.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>

</Project>
"#;

const TEMPLATE_PROGRAM: &str = r#"Console.WriteLine("Hello from LSW (C#)");
"#;

#[derive(Debug)]
pub struct DotnetInitReport {
    pub root: std::path::PathBuf,
    pub created: Vec<std::path::PathBuf>,
}

pub fn init(parent: &std::path::Path, name: Option<&str>) -> Result<DotnetInitReport> {
    if let Some(n) = name {
        crate::envops::validate_name("project", n)?;
    }
    let (root, project_name) = match name {
        Some(n) => (parent.join(n), n.to_owned()),
        None => {
            let n = parent
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or_else(|| Error::InitFailed {
                    path: parent.to_path_buf(),
                    detail: "cannot derive a project name from this directory".into(),
                })?;
            (
                parent.to_path_buf(),
                crate::project::sanitize_project_name(&n),
            )
        }
    };

    if has_dotnet_project(&root) || root.join(lsw_config::PROJECT_MANIFEST).exists() {
        return Err(Error::InitFailed {
            path: root,
            detail: "a .csproj/.sln or lsw.toml already exists here".into(),
        });
    }

    fn write_file(
        root: &std::path::Path,
        rel: &str,
        contents: &str,
        created: &mut Vec<std::path::PathBuf>,
    ) -> Result<()> {
        use std::io::Write;
        let path = root.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io(dir.to_path_buf(), e))?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::InitFailed {
                        path: path.clone(),
                        detail: format!("{rel} already exists; refusing to overwrite"),
                    }
                } else {
                    Error::io(path.clone(), e)
                }
            })?;
        created.push(path.clone());
        file.write_all(contents.as_bytes())
            .map_err(|e| Error::io(path.clone(), e))?;
        Ok(())
    }

    let mut created = Vec::new();
    let manifest_path = root.join(lsw_config::PROJECT_MANIFEST);
    let result: Result<()> = (|| {
        lsw_config::ProjectManifest::new(&project_name).save(&manifest_path)?;
        created.push(manifest_path.clone());
        write_file(
            &root,
            &format!("{project_name}.csproj"),
            TEMPLATE_CSPROJ,
            &mut created,
        )?;
        write_file(&root, "Program.cs", TEMPLATE_PROGRAM, &mut created)?;
        Ok(())
    })();

    match result {
        Ok(()) => Ok(DotnetInitReport { root, created }),
        Err(e) => {
            for path in created.iter().rev() {
                let _ = fs::remove_file(path);
            }
            Err(e)
        }
    }
}

fn has_dotnet_project(root: &std::path::Path) -> bool {
    fs::read_dir(root)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".csproj") || name.ends_with(".sln") || name.ends_with(".fsproj")
            })
        })
        .unwrap_or(false)
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Ok,
    NotConfigured,
    Missing,
}

#[derive(Debug, Serialize)]
pub struct DotnetDoctor {
    pub target: String,
    pub sdk: Check,
    pub runtime_identifier: Check,
    pub self_contained: Check,
    pub runtime_execution: Check,
    pub native_aot: Check,
    pub native_validation: Check,
}

pub fn doctor(env: &Environment) -> Result<DotnetDoctor> {
    let arch = env.manifest.target_arch;
    let rid = dotnet_rid(arch);

    let sdk_ok = which("dotnet").is_some();
    let runtime_ok = env.manifest.runtime.executable.is_file();

    Ok(DotnetDoctor {
        target: rid.unwrap_or("<unsupported>").to_owned(),
        sdk: if sdk_ok { Check::Ok } else { Check::Missing },
        runtime_identifier: if rid.is_some() {
            Check::Ok
        } else {
            Check::Missing
        },
        self_contained: if sdk_ok && rid.is_some() {
            Check::Ok
        } else {
            Check::NotConfigured
        },
        runtime_execution: if runtime_ok {
            Check::Ok
        } else {
            Check::NotConfigured
        },
        native_aot: if which("lld-link").is_some() && which("clang").is_some() {
            Check::Ok
        } else {
            Check::Missing
        },
        native_validation: Check::NotConfigured,
    })
}

pub fn dotnet_rid(arch: TargetArch) -> Option<&'static str> {
    match arch {
        TargetArch::X86_64 => Some("win-x64"),
        TargetArch::X86 => Some("win-x86"),
        TargetArch::Aarch64 | TargetArch::Arm64Ec => Some("win-arm64"),
        TargetArch::Armv7 => None,
    }
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    crate::buildops::which(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_scaffolds_csproj_program_and_lsw_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let report = init(tmp.path(), Some("hello_cs")).unwrap();
        assert!(report.root.join("hello_cs.csproj").is_file());
        assert!(report.root.join("Program.cs").is_file());
        assert!(report.root.join("lsw.toml").is_file());

        let (_, m) = lsw_config::ProjectManifest::discover(&report.root).unwrap();
        assert_eq!(m.project.name, "hello_cs");
        assert!(m.build.is_none());
    }

    #[test]
    fn init_refuses_over_existing_csproj() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.csproj"), b"<Project/>").unwrap();
        assert!(init(tmp.path(), None).is_err());
    }

    #[test]
    fn rid_maps_supported_arches_and_rejects_armv7() {
        assert_eq!(dotnet_rid(TargetArch::X86_64), Some("win-x64"));
        assert_eq!(dotnet_rid(TargetArch::Aarch64), Some("win-arm64"));
        assert_eq!(dotnet_rid(TargetArch::Armv7), None);
    }
}
