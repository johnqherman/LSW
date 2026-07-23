use std::process::ExitCode;

use lsw_core::{Dirs, EnvCreateOptions};

use crate::cli::{EnvCmd, TemplateArg};
use crate::{cwd, project};

pub(crate) fn init(name: &Option<String>, template: &TemplateArg) -> lsw_core::Result<ExitCode> {
    let report = lsw_core::init(&cwd(), name.as_deref(), (*template).into())?;
    println!("Initialized LSW project at {}", report.root.display());
    for f in &report.created {
        println!("  created {}", f.display());
    }
    if let Some(system) = &report.existing_build {
        println!("  detected existing {system} build; template scaffold skipped");
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn env(op: &EnvCmd, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    match op {
        EnvCmd::Create {
            name,
            arch,
            toolchain,
            sdk,
            force,
            expose_home,
        } => {
            println!("Creating environment '{name}' (this initializes a Wine prefix)...");
            let report = lsw_core::env_create(
                dirs,
                &EnvCreateOptions {
                    name: name.clone(),
                    arch: (*arch).into(),
                    toolchain: toolchain.clone(),
                    sdk: sdk.clone(),
                    force: *force,
                    expose_home: *expose_home,
                },
            )?;
            let m = &report.environment.manifest;
            println!("Environment '{name}' ready");
            println!("  arch      {}", m.target_arch);
            println!(
                "  toolchain {} {}",
                m.toolchain.provider, m.toolchain.version
            );
            println!("  runtime   {} {}", m.runtime.provider, m.runtime.version);
            println!("  probe     {}", report.probe.detail);
            if let Ok(mut p) = project() {
                let active_missing = match &p.manifest.environment.name {
                    None => true,
                    Some(active) => matches!(
                        lsw_core::envops::Environment::open(dirs, active),
                        Err(lsw_core::Error::EnvironmentNotFound { .. })
                    ),
                };
                if active_missing {
                    lsw_core::use_environment(dirs, &mut p, name)?;
                    println!(
                        "Project '{}' now uses environment '{name}'",
                        p.manifest.project.name
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        EnvCmd::List => {
            let envs = lsw_core::env_list(dirs)?;
            if envs.is_empty() {
                println!("No environments. Create one with: lsw env create <name>");
            }
            for e in envs {
                println!(
                    "{:<20} {:<8} {:<24} {:<16} {}",
                    e.name,
                    e.arch.to_string(),
                    e.toolchain,
                    e.runtime,
                    if e.healthy { "healthy" } else { "UNHEALTHY" }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        EnvCmd::Remove { name } => {
            lsw_core::env_remove(dirs, name)?;
            println!("Removed environment '{name}'");
            Ok(ExitCode::SUCCESS)
        }

        EnvCmd::Clone { src, dst, force } => {
            let env = lsw_core::clone_env(dirs, src, dst, *force)?;
            println!("Cloned environment '{src}' to '{}'", env.name);
            Ok(ExitCode::SUCCESS)
        }

        EnvCmd::Restore { name } => {
            let p = project()?;
            println!("Restoring environment '{name}' from lsw.lock...");
            let report = lsw_core::env_restore(dirs, &p, name)?;
            let m = &report.environment.manifest;
            println!("Environment '{name}' restored and verified against lsw.lock");
            println!("  arch      {}", m.target_arch);
            println!(
                "  toolchain {} {}",
                m.toolchain.provider, m.toolchain.version
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn use_env(name: &str, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let mut p = project()?;
    lsw_core::use_environment(dirs, &mut p, name)?;
    println!(
        "Project '{}' now uses environment '{name}'",
        p.manifest.project.name
    );
    Ok(ExitCode::SUCCESS)
}
