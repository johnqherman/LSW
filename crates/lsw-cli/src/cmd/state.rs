use std::process::ExitCode;

use lsw_core::Dirs;

use crate::active_env;
use crate::cli::{Format, RegistryCmd, ServiceCmd};

pub(crate) fn registry(op: &RegistryCmd, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    match op {
        RegistryCmd::Get { key, value } => {
            lsw_core::registryops::get(&env, key, value.as_deref())?;
        }
        RegistryCmd::Set {
            key,
            value,
            data,
            kind,
        } => {
            lsw_core::registryops::set(&env, key, value, data, kind)?;
            println!("set {key}\\{value}");
        }
        RegistryCmd::Export { key, file } => {
            lsw_core::registryops::export(&env, key, file)?;
            println!("exported {key} to {}", file.display());
        }
        RegistryCmd::Import { file } => {
            lsw_core::registryops::import(&env, file)?;
            println!("imported {}", file.display());
        }
        RegistryCmd::Seed => {
            let n = lsw_core::registryops::seed(&env, &p)?;
            println!("applied {n} registry seed(s) to '{}'", env.name);
        }
        RegistryCmd::Reset => {
            lsw_core::registryops::reset(&env)?;
            println!("registry reset to prefix defaults for '{}'", env.name);
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn ps(all: bool, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let (_p, env) = active_env(dirs)?;
    let mut processes = lsw_core::psops::ps(&env)?;
    if !all {
        processes.retain(|p| !lsw_core::psops::is_wine_infrastructure(&p.command));
    }
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&processes).expect("serializes")
        );
    } else if processes.is_empty() {
        println!("No processes running in environment '{}'", env.name);
        if !all {
            println!("(wine infrastructure is hidden; lsw ps --all shows it)");
        }
    } else {
        println!("{:<8} COMMAND", "PID");
        for p in processes {
            println!("{:<8} {}", p.pid, p.command);
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn kill(pid: &Option<u32>, all: &bool, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let (_p, env) = active_env(dirs)?;
    if *all {
        lsw_core::psops::kill_all(&env)?;
        println!("environment '{}' shut down", env.name);
    } else if let Some(pid) = pid {
        lsw_core::psops::kill(&env, *pid)?;
        println!("sent SIGTERM to {pid}");
    } else {
        eprintln!("usage: lsw kill <pid> | lsw kill --all");
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn service(op: &ServiceCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let (_p, env) = active_env(dirs)?;
    match op {
        ServiceCmd::Create { name, bin } => {
            lsw_core::serviceops::create(&env, name, bin)?;
            println!("created service '{name}'");
        }
        ServiceCmd::Start { name } => {
            lsw_core::serviceops::start(&env, name)?;
            println!("started service '{name}'");
        }
        ServiceCmd::Stop { name } => {
            lsw_core::serviceops::stop(&env, name)?;
            println!("stopped service '{name}'");
        }
        ServiceCmd::Query { name } => {
            let status = lsw_core::serviceops::query(&env, name)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).expect("serializes")
                );
            } else {
                println!("{:<24} {}", status.name, status.state);
            }
        }
        ServiceCmd::Delete { name } => {
            lsw_core::serviceops::delete(&env, name)?;
            println!("deleted service '{name}'");
        }
    }
    Ok(ExitCode::SUCCESS)
}
