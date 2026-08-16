use std::path::Path;

use serde::Serialize;

use crate::envops::{self, EnvCreateOptions};
use crate::error::{Error, Result};
use crate::project::Project;
use lsw_config::{Dirs, TargetArch};

#[derive(Debug, Serialize)]
pub struct SetupReport {
    pub project_name: String,
    pub project_root: String,
    pub build_system: Option<String>,
    pub manifest_created: bool,
    pub environment: String,
    pub environment_created: bool,
    pub toolchain: String,
    pub runtime: String,
    pub arch_mismatch: Option<String>,
}

pub const DEFAULT_ENV_NAME: &str = "windows-x64";

pub fn default_env_name(arch: TargetArch) -> &'static str {
    match arch {
        TargetArch::X86_64 => DEFAULT_ENV_NAME,
        TargetArch::X86 => "windows-x86",
        TargetArch::Aarch64 => "windows-arm64",
        TargetArch::Armv7 => "windows-arm",
        TargetArch::Arm64Ec => "windows-arm64ec",
    }
}

pub fn setup(dirs: &Dirs, start: &Path) -> Result<SetupReport> {
    let (mut project, manifest_created) = discover_or_create(start)?;
    let build_system =
        crate::buildops::detect_build_system(&project.root).map(|s| s.label().to_owned());

    let arch = project.manifest.target.arch;
    let env_name = default_env_name(arch);
    let mut environment_created = false;
    let env = match envops::resolve_active(dirs, &project) {
        Ok(env) => env,
        Err(Error::NoActiveEnvironment | Error::EnvironmentNotFound { .. }) => {
            match envops::Environment::open(dirs, env_name) {
                Ok(env) => env,
                Err(Error::EnvironmentNotFound { .. }) => {
                    let report = envops::create(
                        dirs,
                        &EnvCreateOptions {
                            name: env_name.to_owned(),
                            arch,
                            toolchain: None,
                            sdk: None,
                            force: false,
                            expose_home: false,
                        },
                    )?;
                    environment_created = true;
                    report.environment
                }
                Err(e) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };

    if project.manifest.environment.name.as_deref() != Some(&env.name) {
        envops::use_environment(dirs, &mut project, &env.name)?;
    }

    let m = &env.manifest;
    let arch_mismatch = (m.target_arch != arch).then(|| {
        format!(
            "lsw.toml sets [target] arch = \"{arch}\" but environment '{}' targets {}; create one with: lsw env create {} --arch {arch}",
            env.name, m.target_arch, default_env_name(arch),
        )
    });
    Ok(SetupReport {
        project_name: project.manifest.project.name.clone(),
        project_root: project.root.display().to_string(),
        build_system,
        manifest_created,
        environment: env.name.clone(),
        environment_created,
        toolchain: format!("{} {}", m.toolchain.provider, m.toolchain.version),
        runtime: format!("{} {}", m.runtime.provider, m.runtime.version),
        arch_mismatch,
    })
}

fn discover_or_create(start: &Path) -> Result<(Project, bool)> {
    match Project::discover(start) {
        Ok(p) => Ok((p, false)),
        Err(Error::Config(lsw_config::ConfigError::ProjectNotFound { .. })) => {
            let name = start
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or_else(|| Error::InitFailed {
                    path: start.to_path_buf(),
                    detail: "cannot derive a project name from this directory".into(),
                })?;
            let name = crate::project::sanitize_project_name(&name);
            lsw_config::ProjectManifest::new(&name)
                .save_new(&start.join(lsw_config::PROJECT_MANIFEST))?;
            crate::project::ensure_gitignore(start)?;
            Ok((Project::discover(start)?, true))
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_env_name_x86_64() {
        assert_eq!(default_env_name(TargetArch::X86_64), "windows-x64");
    }

    #[test]
    fn default_env_name_x86() {
        assert_eq!(default_env_name(TargetArch::X86), "windows-x86");
    }

    #[test]
    fn default_env_name_aarch64() {
        assert_eq!(default_env_name(TargetArch::Aarch64), "windows-arm64");
    }

    #[test]
    fn default_env_name_armv7() {
        assert_eq!(default_env_name(TargetArch::Armv7), "windows-arm");
    }

    #[test]
    fn default_env_name_arm64ec() {
        assert_eq!(default_env_name(TargetArch::Arm64Ec), "windows-arm64ec");
    }

    #[test]
    fn default_env_name_constant_matches_x64() {
        assert_eq!(DEFAULT_ENV_NAME, default_env_name(TargetArch::X86_64));
    }

    #[test]
    fn setup_report_serializes() {
        let r = SetupReport {
            project_name: "test".into(),
            project_root: "/tmp/test".into(),
            build_system: Some("cmake".into()),
            manifest_created: true,
            environment: "windows-x64".into(),
            environment_created: true,
            toolchain: "llvm-mingw 20241231".into(),
            runtime: "wine 10.0".into(),
            arch_mismatch: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"project_name\":\"test\""));
        assert!(json.contains("\"manifest_created\":true"));
    }
}
