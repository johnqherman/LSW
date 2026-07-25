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
}

pub const DEFAULT_ENV_NAME: &str = "windows-x64";

pub fn setup(dirs: &Dirs, start: &Path) -> Result<SetupReport> {
    let (mut project, manifest_created) = discover_or_create(start)?;
    let build_system =
        crate::buildops::detect_build_system(&project.root).map(|s| format!("{s:?}"));

    let mut environment_created = false;
    let env = match envops::resolve_active(dirs, &project) {
        Ok(env) => env,
        Err(Error::NoActiveEnvironment | Error::EnvironmentNotFound { .. }) => {
            match envops::Environment::open(dirs, DEFAULT_ENV_NAME) {
                Ok(env) => env,
                Err(Error::EnvironmentNotFound { .. }) => {
                    let report = envops::create(
                        dirs,
                        &EnvCreateOptions {
                            name: DEFAULT_ENV_NAME.to_owned(),
                            arch: TargetArch::X86_64,
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
    Ok(SetupReport {
        project_name: project.manifest.project.name.clone(),
        project_root: project.root.display().to_string(),
        build_system,
        manifest_created,
        environment: env.name.clone(),
        environment_created,
        toolchain: format!("{} {}", m.toolchain.provider, m.toolchain.version),
        runtime: format!("{} {}", m.runtime.provider, m.runtime.version),
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
            Ok((Project::discover(start)?, true))
        }
        Err(e) => Err(e),
    }
}
