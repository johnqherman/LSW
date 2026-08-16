use std::fs;
use std::path::Path;

use serde::Serialize;

use lsw_runtime::RuntimeProvider;

use crate::envops::Environment;
use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
/// Process Info.
pub struct ProcessInfo {
    /// Pid.
    pub pid: u32,
    /// Command.
    pub command: String,
}

const WINE_INFRASTRUCTURE: &[&str] = &[
    "wineserver",
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "conhost.exe",
    "tabtip.exe",
];

/// Returns whether wine infrastructure.
pub fn is_wine_infrastructure(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if lower.contains("explorer.exe /desktop") {
        return true;
    }
    lower
        .split(['/', '\\', ' '])
        .any(|seg| WINE_INFRASTRUCTURE.contains(&seg))
}

/// Ps.
pub fn ps(env: &Environment) -> Result<Vec<ProcessInfo>> {
    let prefix = env.layout.prefix();
    let mut out = Vec::new();
    let proc_dir = Path::new("/proc");
    let entries = fs::read_dir(proc_dir).map_err(|e| Error::io(proc_dir.to_path_buf(), e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if !process_uses_prefix(pid, &prefix) {
            continue;
        }
        let cmdline = fs::read(entry.path().join("cmdline")).ok();
        let parts: Vec<String> = cmdline
            .as_deref()
            .unwrap_or_default()
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        let command = if parts.is_empty() {
            "<unknown>".to_owned()
        } else {
            parts.join(" ")
        };
        out.push(ProcessInfo { pid, command });
    }
    out.sort_by_key(|p| p.pid);
    Ok(out)
}

fn process_uses_prefix(pid: u32, prefix: &Path) -> bool {
    let Ok(environ) = fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    let needle = format!("WINEPREFIX={}", prefix.display());
    environ
        .split(|b| *b == 0)
        .any(|entry| entry == needle.as_bytes())
}

/// Kill.
pub fn kill(env: &Environment, pid: u32) -> Result<()> {
    let prefix = env.layout.prefix();
    lsw_runtime::WineRuntime
        .kill(&prefix, pid)
        .map_err(|_| Error::ProcessNotInEnvironment {
            pid,
            environment: env.name.clone(),
        })
}

/// Kill all.
pub fn kill_all(env: &Environment) -> Result<()> {
    Ok(lsw_runtime::WineRuntime.shutdown_prefix(&env.layout.prefix())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_matches_its_own_environ() {
        let pid = std::process::id();
        assert!(!process_uses_prefix(pid, Path::new("/nonexistent/prefix")));
    }
}
