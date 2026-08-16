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
/// Dotnet Init Report.
pub struct DotnetInitReport {
    /// Root.
    pub root: std::path::PathBuf,
    /// Created.
    pub created: Vec<std::path::PathBuf>,
}

/// Init.
pub fn init(parent: &std::path::Path, name: Option<&str>) -> Result<DotnetInitReport> {
    if let Some(n) = name {
        crate::envops::validate_name("project", n)?;
    }
    let (root, project_name) = if let Some(n) = name { (parent.join(n), n.to_owned()) } else {
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
    };

    if has_dotnet_project(&root) || root.join(lsw_config::PROJECT_MANIFEST).exists() {
        return Err(Error::InitFailed {
            path: root,
            detail: "a .csproj/.sln or lsw.toml already exists here".into(),
        });
    }

    let mut created = Vec::new();
    let manifest_path = root.join(lsw_config::PROJECT_MANIFEST);
    let result: Result<()> = (|| {
        lsw_config::ProjectManifest::new(&project_name).save_new(&manifest_path)?;
        created.push(manifest_path.clone());
        crate::project::scaffold_write(
            &root,
            &format!("{project_name}.csproj"),
            TEMPLATE_CSPROJ,
            &mut created,
        )?;
        crate::project::scaffold_write(&root, "Program.cs", TEMPLATE_PROGRAM, &mut created)?;
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

pub(crate) fn has_dotnet_project(root: &std::path::Path) -> bool {
    fs::read_dir(root).is_ok_and(|entries| {
        entries.flatten().take(1_000_000).any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".csproj") || name.ends_with(".sln") || name.ends_with(".fsproj")
        })
    })
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// Check.
pub enum Check {
    /// Ok.
    Ok,
    /// Not Configured.
    NotConfigured,
    /// Missing.
    Missing,
}

#[derive(Debug, Serialize)]
/// Dotnet Doctor.
pub struct DotnetDoctor {
    /// Target.
    pub target: String,
    /// Sdk.
    pub sdk: Check,
    /// Runtime identifier.
    pub runtime_identifier: Check,
    /// Self contained.
    pub self_contained: Check,
    /// Runtime execution.
    pub runtime_execution: Check,
    /// Native aot.
    pub native_aot: Check,
    /// Native validation.
    pub native_validation: Check,
}

/// Doctor.
pub fn doctor(env: &Environment) -> Result<DotnetDoctor> {
    let arch = env.manifest.target_arch;
    let rid = dotnet_rid(arch);

    let sdk_ok = crate::buildops::which("dotnet").is_some();
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
        native_aot: if crate::buildops::which("lld-link").is_some()
            && crate::buildops::which("clang").is_some()
        {
            Check::Ok
        } else {
            Check::Missing
        },
        native_validation: Check::NotConfigured,
    })
}

/// Dotnet rid.
pub fn dotnet_rid(arch: TargetArch) -> Option<&'static str> {
    match arch {
        TargetArch::X86_64 => Some("win-x64"),
        TargetArch::X86 => Some("win-x86"),
        TargetArch::Aarch64 | TargetArch::Arm64Ec => Some("win-arm64"),
        TargetArch::Armv7 => None,
    }
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
