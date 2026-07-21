use std::path::{Path, PathBuf};

use lsw_runtime::{ExecutionRequest, RuntimeProvider, WineRuntime};

use crate::envops::Environment;
use crate::error::{Error, Result};

fn z_path(path: &Path) -> Result<String> {
    let absolute = std::path::absolute(path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    let text = absolute.to_str().ok_or_else(|| Error::NotExecutable {
        program: absolute.clone(),
        detail: "path is not valid UTF-8".into(),
    })?;
    Ok(format!("Z:{}", text.replace('/', "\\")))
}

fn run_registry_tool(env: &Environment, program: &str, args: Vec<String>) -> Result<()> {
    let status = WineRuntime.execute(&ExecutionRequest {
        program: PathBuf::from(program),
        args,
        prefix: env.layout.prefix(),
        cwd: None,
        env: Vec::new(),
        sandbox: None,
    })?;
    if !status.success() {
        return Err(Error::RegistryOperationFailed {
            code: status.code(),
        });
    }
    Ok(())
}

pub fn get(env: &Environment, key: &str, value: Option<&str>) -> Result<()> {
    let mut args = vec!["query".to_owned(), key.to_owned()];
    if let Some(v) = value {
        args.push("/v".to_owned());
        args.push(v.to_owned());
    }
    run_registry_tool(env, "reg.exe", args)
}

pub fn set(env: &Environment, key: &str, value: &str, data: &str, kind: &str) -> Result<()> {
    run_registry_tool(
        env,
        "reg.exe",
        vec![
            "add".to_owned(),
            key.to_owned(),
            "/v".to_owned(),
            value.to_owned(),
            "/t".to_owned(),
            kind.to_owned(),
            "/d".to_owned(),
            data.to_owned(),
            "/f".to_owned(),
        ],
    )
}

pub fn export(env: &Environment, key: &str, file: &Path) -> Result<()> {
    run_registry_tool(
        env,
        "reg.exe",
        vec![
            "export".to_owned(),
            key.to_owned(),
            z_path(file)?,
            "/y".to_owned(),
        ],
    )
}

pub fn import(env: &Environment, file: &Path) -> Result<()> {
    if !file.is_file() {
        return Err(Error::NotExecutable {
            program: file.to_path_buf(),
            detail: "registry file not found".into(),
        });
    }
    run_registry_tool(env, "regedit", vec!["/S".to_owned(), z_path(file)?])
}

pub fn reset(env: &Environment) -> Result<()> {
    let prefix = env.layout.prefix();
    WineRuntime.shutdown_prefix(&prefix)?;
    for name in ["system.reg", "user.reg", "userdef.reg"] {
        let file = prefix.join(name);
        if file.is_file() {
            std::fs::remove_file(&file).map_err(|e| Error::io(file.clone(), e))?;
        }
    }
    Ok(WineRuntime.prepare(&prefix)?)
}
