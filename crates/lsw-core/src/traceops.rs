use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

const TRACE_TIMEOUT: Duration = Duration::from_secs(120);
const TRACE_MAX_OUTPUT: usize = 32 * 1024 * 1024;

fn drain_capped(mut reader: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() < TRACE_MAX_OUTPUT {
                        let take = (TRACE_MAX_OUTPUT - buf.len()).min(n);
                        buf.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
        let _ = tx.send(buf);
    });
    rx
}

#[derive(Debug, Serialize)]
pub struct TraceReport {
    pub imported_dlls: Vec<String>,
    pub loaded_dlls: Vec<String>,
    pub observed_calls: Vec<String>,
    pub registry_access: Vec<String>,
    pub filesystem_access: Vec<String>,
    pub unsupported: Vec<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Default)]
pub struct TraceOptions {
    pub relay: bool,
}

pub fn trace(
    env: &Environment,
    program: &Path,
    args: &[String],
    opts: &TraceOptions,
) -> Result<TraceReport> {
    if !program.is_file() {
        return Err(Error::NotExecutable {
            program: program.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let program = std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;

    let imported_dlls = lsw_pe::imports(&program)?;

    let wine = find_wine().ok_or_else(|| Error::ToolMissing {
        tool: "wine".into(),
        fix: "install wine".into(),
    })?;

    let channels = if opts.relay {
        "+loaddll,+reg,+file,+relay,fixme-all"
    } else {
        "+loaddll,+reg,+file,fixme-all"
    };

    let mut child = Command::new(&wine)
        .arg(&program)
        .args(args)
        .env("WINEPREFIX", env.layout.prefix())
        .env("WINEDEBUG", channels)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::io(wine.clone(), e))?;
    let out_rx = child.stdout.take().map(drain_capped);
    let err_rx = child.stderr.take().map(drain_capped);
    let deadline = Instant::now() + TRACE_TIMEOUT;
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (None, true);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break (None, false),
        }
    };
    let stdout_bytes = out_rx.and_then(|rx| rx.recv().ok()).unwrap_or_default();
    let stderr_bytes = err_rx.and_then(|rx| rx.recv().ok()).unwrap_or_default();

    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let parsed = parse_wine_trace(&stderr);

    eprint!("{}", String::from_utf8_lossy(&stdout_bytes));
    if timed_out {
        eprintln!(
            "lsw: trace timed out after {}s and was killed",
            TRACE_TIMEOUT.as_secs()
        );
    }

    Ok(TraceReport {
        imported_dlls,
        loaded_dlls: parsed.loaded,
        observed_calls: parsed.calls,
        registry_access: parsed.registry,
        filesystem_access: parsed.filesystem,
        unsupported: parsed.unsupported,
        exit_code: status.and_then(|s| s.code()),
    })
}

struct ParsedTrace {
    loaded: Vec<String>,
    calls: Vec<String>,
    registry: Vec<String>,
    filesystem: Vec<String>,
    unsupported: Vec<String>,
}

fn extract_channel_op(line: &str, tag: &str) -> Option<String> {
    let after = line.split_once(tag).map(|(_, r)| r)?.trim_start();
    let op: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if op.is_empty() { None } else { Some(op) }
}

fn parse_wine_trace(stderr: &str) -> ParsedTrace {
    let mut loaded = BTreeSet::new();
    let mut calls = BTreeSet::new();
    let mut registry = BTreeSet::new();
    let mut filesystem = BTreeSet::new();
    let mut unsupported = BTreeSet::new();

    for line in stderr.lines() {
        let line = line.trim();

        if line.contains("trace:loaddll:") {
            if let Some(name) = extract_module_name(line) {
                loaded.insert(name);
            }
            continue;
        }

        if line.contains("trace:reg:") {
            if let Some(op) = extract_channel_op(line, "trace:reg:") {
                registry.insert(op);
            }
            continue;
        }

        if line.contains("trace:file:") {
            if let Some(op) = extract_channel_op(line, "trace:file:") {
                filesystem.insert(op);
            }
            continue;
        }

        if let Some(after) = line.split_once("Call ").map(|(_, r)| r) {
            if let Some(call) = extract_relay_call(after) {
                calls.insert(call);
            }
            continue;
        }

        let lower = line.to_ascii_lowercase();
        let unimplemented = lower.contains("not implemented")
            || lower.contains("unimplemented")
            || lower.contains("no implementation for");
        if unimplemented && let Some(sym) = extract_unimplemented(line) {
            unsupported.insert(sym);
        }
    }

    ParsedTrace {
        loaded: loaded.into_iter().collect(),
        calls: calls.into_iter().collect(),
        registry: registry.into_iter().collect(),
        filesystem: filesystem.into_iter().collect(),
        unsupported: unsupported.into_iter().collect(),
    }
}

fn extract_module_name(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    let path = &rest[..end];
    let base = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    if base.ends_with(".dll") {
        Some(base)
    } else {
        None
    }
}

fn extract_relay_call(fragment: &str) -> Option<String> {
    let head = fragment.split('(').next()?.trim();
    let (module, func) = head.split_once('.')?;
    if module.is_empty() || func.is_empty() {
        return None;
    }
    Some(format!("{}!{}", module.to_ascii_lowercase(), func))
}

fn extract_unimplemented(line: &str) -> Option<String> {
    if let Some(after) = line.split("for ").nth(1) {
        let sym = after
            .split_whitespace()
            .next()?
            .trim_end_matches([',', '.']);
        if let Some((module, func)) = sym.rsplit_once('.') {
            let module = module
                .strip_suffix(".dll")
                .unwrap_or(module)
                .to_ascii_lowercase();
            if !module.is_empty() && !func.is_empty() {
                return Some(format!("{module}!{func}"));
            }
        }
    }
    None
}

fn find_wine() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("wine"))
        .find(|c| c.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loaddll_lines() {
        let stderr = r#"
002c:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\kernel32.dll" at 0x7b00: builtin
002c:trace:loaddll:build_ntdll_module Loaded L"C:\\windows\\system32\\ntdll.dll" at 0x7c00: builtin
irrelevant line
"#;
        let p = parse_wine_trace(stderr);
        assert_eq!(p.loaded, vec!["kernel32.dll", "ntdll.dll"]);
    }

    #[test]
    fn parses_relay_calls() {
        let stderr = "0024:Call kernel32.CreateFileW(0x1,0x2) ret=00401000\n\
                      0024:Call user32.MessageBoxA(0,\"hi\") ret=0";
        let p = parse_wine_trace(stderr);
        assert!(p.calls.contains(&"kernel32!CreateFileW".to_owned()));
        assert!(p.calls.contains(&"user32!MessageBoxA".to_owned()));
    }

    #[test]
    fn parses_unimplemented() {
        let stderr =
            "err:module:import_dll No implementation for dxgi.dll.SomeNewFn imported from ...";
        let p = parse_wine_trace(stderr);
        assert_eq!(p.unsupported, vec!["dxgi!SomeNewFn"]);
    }

    #[test]
    fn categorizes_registry_and_filesystem_access() {
        let stderr = "0024:trace:reg:RegOpenKeyExW (HKLM,...)\n\
                      0024:trace:reg:RegQueryValueExW (...)\n\
                      0024:trace:file:CreateFileW L\"C:\\\\x\"\n\
                      0024:trace:file:CreateFileW L\"C:\\\\y\"";
        let p = parse_wine_trace(stderr);
        assert_eq!(p.registry, vec!["RegOpenKeyExW", "RegQueryValueExW"]);
        assert_eq!(p.filesystem, vec!["CreateFileW"]);
    }

    #[test]
    fn ignores_unrelated_output() {
        let p = parse_wine_trace("just some program stdout\nnothing to see");
        assert!(p.loaded.is_empty() && p.calls.is_empty() && p.unsupported.is_empty());
    }
}
