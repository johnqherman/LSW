use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
pub struct TraceReport {
    pub imported_dlls: Vec<String>,
    pub loaded_dlls: Vec<String>,
    pub observed_calls: Vec<String>,
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
        "+loaddll,+relay,fixme-all"
    } else {
        "+loaddll,fixme-all"
    };

    let output = Command::new(&wine)
        .arg(&program)
        .args(args)
        .env("WINEPREFIX", env.layout.prefix())
        .env("WINEDEBUG", channels)
        .output()
        .map_err(|e| Error::io(wine.clone(), e))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed = parse_wine_trace(&stderr);

    print!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(TraceReport {
        imported_dlls,
        loaded_dlls: parsed.loaded,
        observed_calls: parsed.calls,
        unsupported: parsed.unsupported,
        exit_code: output.status.code(),
    })
}

struct ParsedTrace {
    loaded: Vec<String>,
    calls: Vec<String>,
    unsupported: Vec<String>,
}

fn parse_wine_trace(stderr: &str) -> ParsedTrace {
    let mut loaded = BTreeSet::new();
    let mut calls = BTreeSet::new();
    let mut unsupported = BTreeSet::new();

    for line in stderr.lines() {
        let line = line.trim();

        if line.contains("trace:loaddll:") {
            if let Some(name) = extract_module_name(line) {
                loaded.insert(name);
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
    fn ignores_unrelated_output() {
        let p = parse_wine_trace("just some program stdout\nnothing to see");
        assert!(p.loaded.is_empty() && p.calls.is_empty() && p.unsupported.is_empty());
    }
}
