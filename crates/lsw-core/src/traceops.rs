use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

#[derive(Debug, Default)]
pub struct TraceOptions {
    pub full: bool,
}

#[derive(Debug, Serialize)]
pub struct TraceReport {
    pub exit_code: Option<i32>,
    pub log: PathBuf,
    pub loaded_dlls: Vec<String>,
    pub top_calls: Vec<(String, u64)>,
}

pub fn trace(env: &Environment, program: &Path, args: &[String], opts: &TraceOptions) -> Result<TraceReport> {
    if !program.is_file() {
        return Err(Error::NotExecutable {
            program: program.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let program = std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;

    let runtime = &env.manifest.runtime.executable;
    let channels = if opts.full {
        "+loaddll,+relay"
    } else {
        "+loaddll"
    };

    let logs_dir = env.layout.logs();
    std::fs::create_dir_all(&logs_dir).map_err(|e| Error::io(logs_dir.clone(), e))?;
    let stem = program
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "trace".into());
    let log_path = logs_dir.join(format!("trace-{stem}.log"));

    let mut command = Command::new(runtime);
    command.arg(&program).args(args);
    for var in [
        "WINEPREFIX",
        "WINEDEBUG",
        "WINEDLLOVERRIDES",
        "WINEARCH",
        "WINESERVER",
    ] {
        command.env_remove(var);
    }
    command
        .env("WINEPREFIX", env.layout.prefix())
        .env("WINEDEBUG", channels)
        .env("WINEDLLOVERRIDES", "winemenubuilder.exe=d")
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped());

    let output = command.output().map_err(|e| Error::io(runtime.clone(), e))?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut log = std::fs::File::create(&log_path).map_err(|e| Error::io(log_path.clone(), e))?;
    log.write_all(stderr.as_bytes())
        .map_err(|e| Error::io(log_path.clone(), e))?;

    Ok(TraceReport {
        exit_code: output.status.code(),
        log: log_path,
        loaded_dlls: parse_loaded_dlls(&stderr),
        top_calls: parse_relay_calls(&stderr, 30),
    })
}

fn parse_loaded_dlls(stderr: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for line in stderr.lines() {
        if !line.contains(":loaddll:") {
            continue;
        }
        let Some(start) = line.find("Loaded L\"") else {
            continue;
        };
        let rest = &line[start + "Loaded L\"".len()..];
        let Some(end) = rest.find('"') else { continue };
        let path = rest[..end].replace("\\\\", "\\");
        let name = path.rsplit('\\').next().unwrap_or(&path).to_owned();
        if seen.insert(name.to_ascii_lowercase()
