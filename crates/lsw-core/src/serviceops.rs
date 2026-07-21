use std::path::PathBuf;

use serde::Serialize;

use lsw_runtime::{RuntimeProvider, WineRuntime};

use crate::envops::Environment;
use crate::error::{Error, Result};

fn sc(env: &Environment, args: Vec<String>) -> Result<std::process::Output> {
    let wine = WineRuntime.resolve()?.executable;
    let mut command = std::process::Command::new(wine);
    lsw_runtime::scrub_wine_env(&mut command);
    for (k, v) in lsw_runtime::base_env(&env.layout.prefix()) {
        command.env(k, v);
    }
    command.arg("sc.exe").args(&args);
    command
        .output()
        .map_err(|e| Error::io(PathBuf::from("sc.exe"), e))
}

#[derive(Debug, Serialize)]
pub struct ServiceStatus {
    pub name: String,
    pub state: String,
}

pub fn create(env: &Environment, name: &str, bin_path: &str) -> Result<()> {
    let out = sc(
        env,
        vec!["create".into(), name.into(), format!("binPath= {bin_path}")],
    )?;
    finish("create", name, &out)
}

pub fn start(env: &Environment, name: &str) -> Result<()> {
    let out = sc(env, vec!["start".into(), name.into()])?;
    finish("start", name, &out)
}

pub fn stop(env: &Environment, name: &str) -> Result<()> {
    let out = sc(env, vec!["stop".into(), name.into()])?;
    finish("stop", name, &out)
}

pub fn delete(env: &Environment, name: &str) -> Result<()> {
    let out = sc(env, vec!["delete".into(), name.into()])?;
    finish("delete", name, &out)
}

pub fn query(env: &Environment, name: &str) -> Result<ServiceStatus> {
    let out = sc(env, vec!["query".into(), name.into()])?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() && !stdout.contains("STATE") {
        return Err(Error::ServiceFailed {
            op: "query".into(),
            name: name.into(),
            detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(ServiceStatus {
        name: name.to_owned(),
        state: parse_state(&stdout).unwrap_or_else(|| "UNKNOWN".to_owned()),
    })
}

fn parse_state(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.split_once("STATE") {
            return rest.1.split_whitespace().last().map(str::to_owned);
        }
    }
    None
}

fn finish(op: &str, name: &str, out: &std::process::Output) -> Result<()> {
    if out.status.success() {
        return Ok(());
    }
    let detail = {
        let so = String::from_utf8_lossy(&out.stdout);
        let se = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{}\n{}", so.trim(), se.trim());
        combined.trim().to_owned()
    };
    Err(Error::ServiceFailed {
        op: op.to_owned(),
        name: name.to_owned(),
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_state_reads_running() {
        let sample = "SERVICE_NAME: Demo\n        STATE              : 4  RUNNING\n        WIN32_EXIT_CODE : 0";
        assert_eq!(parse_state(sample).as_deref(), Some("RUNNING"));
    }

    #[test]
    fn parse_state_reads_stopped_and_missing() {
        assert_eq!(
            parse_state("        STATE              : 1  STOPPED").as_deref(),
            Some("STOPPED")
        );
        assert_eq!(parse_state("no state here"), None);
    }
}
