use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

const TRACE_TIMEOUT: Duration = Duration::from_mins(2);
const TRACE_MAX_OUTPUT: usize = 32 * 1024 * 1024;
const TRACE_MAX_EVENTS: usize = 100_000;
const TRACE_MAX_FIELD: usize = 512;

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TraceEventKind {
    Dll,
    Registry,
    Filesystem,
    Call,
    Unsupported,
}

#[derive(Debug, Serialize)]
pub struct TraceEvent {
    pub at_ms: u64,
    pub kind: TraceEventKind,
    pub verb: String,
    pub path_or_key: String,
}

#[derive(Debug, Serialize)]
pub struct TraceReport {
    pub imported_dlls: Vec<String>,
    pub loaded_dlls: Vec<String>,
    pub observed_calls: Vec<String>,
    pub registry_access: Vec<String>,
    pub filesystem_access: Vec<String>,
    pub unsupported: Vec<String>,
    pub timeline: Vec<TraceEvent>,
    pub timeline_truncated: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Default)]
pub struct TraceOptions {
    pub relay: bool,
    pub filter: Option<String>,
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

    let wine = crate::buildops::which("wine").ok_or_else(|| Error::ToolMissing {
        tool: "wine".into(),
        fix: "install wine".into(),
    })?;

    let channels = if opts.relay {
        "+timestamp,+loaddll,+reg,+file,+relay,fixme-all"
    } else {
        "+timestamp,+loaddll,+reg,+file,fixme-all"
    };

    let mut cmd = Command::new(&wine);
    cmd.arg(&program)
        .args(args)
        .env("WINEPREFIX", env.layout.prefix())
        .env("WINEDEBUG", channels);
    let capped =
        lsw_toolchain::capped_output_with(&mut cmd, TRACE_MAX_OUTPUT as u64, Some(TRACE_TIMEOUT))
            .map_err(|e| Error::io(wine.clone(), e))?;
    let (status, timed_out) = if capped.timed_out {
        (None, true)
    } else {
        (Some(capped.status), false)
    };
    let stdout_bytes = capped.stdout;
    let stderr_bytes = capped.stderr;

    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let parsed = parse_wine_trace(&stderr, opts.filter.as_deref());

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
        timeline: parsed.timeline,
        timeline_truncated: parsed.timeline_truncated,
        exit_code: status.and_then(|s| s.code()),
    })
}

struct ParsedTrace {
    loaded: Vec<String>,
    calls: Vec<String>,
    registry: Vec<String>,
    filesystem: Vec<String>,
    unsupported: Vec<String>,
    timeline: Vec<TraceEvent>,
    timeline_truncated: bool,
}

pub(crate) struct Glob {
    p: Vec<char>,
}

impl Glob {
    pub(crate) fn new(pattern: &str) -> Self {
        Self {
            p: pattern.to_ascii_lowercase().chars().collect(),
        }
    }

    pub(crate) fn matches(&self, text: &str) -> bool {
        glob_chars(&self.p, text)
    }
}

#[cfg(test)]
fn glob_match(pattern: &str, text: &str) -> bool {
    Glob::new(pattern).matches(text)
}

fn glob_chars(p: &[char], text: &str) -> bool {
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn line_timestamp(line: &str) -> (Option<u64>, &str) {
    let Some((head, rest)) = line.split_once(':') else {
        return (None, line);
    };
    let Some((secs, millis)) = head.split_once('.') else {
        return (None, line);
    };
    let numeric = !secs.is_empty()
        && !millis.is_empty()
        && secs.chars().all(|c| c.is_ascii_digit())
        && millis.chars().all(|c| c.is_ascii_digit());
    match (numeric, secs.parse::<u64>(), millis.parse::<u64>()) {
        (true, Ok(s), Ok(m)) => (Some(s * 1000 + m), rest),
        _ => (None, line),
    }
}

fn extract_quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].chars().take(TRACE_MAX_FIELD).collect())
}

fn parse_wine_trace(stderr: &str, filter: Option<&str>) -> ParsedTrace {
    let filter = filter.map(Glob::new);
    let mut loaded = BTreeSet::new();
    let mut calls = BTreeSet::new();
    let mut registry = BTreeSet::new();
    let mut filesystem = BTreeSet::new();
    let mut unsupported = BTreeSet::new();
    let mut timeline = Vec::new();
    let mut timeline_truncated = false;
    let mut first_ts: Option<u64> = None;

    let mut push_event = |at: Option<u64>,
                          first_ts: &mut Option<u64>,
                          kind: TraceEventKind,
                          verb: String,
                          path_or_key: String| {
        if let Some(f) = &filter
            && !f.matches(&path_or_key)
            && !f.matches(&verb)
        {
            return;
        }
        if timeline.len() >= TRACE_MAX_EVENTS {
            timeline_truncated = true;
            return;
        }
        let at_ms = match at {
            Some(t) => {
                let base = *first_ts.get_or_insert(t);
                t.saturating_sub(base)
            }
            None => timeline
                .last()
                .map(|e: &TraceEvent| e.at_ms)
                .unwrap_or_default(),
        };
        timeline.push(TraceEvent {
            at_ms,
            kind,
            verb,
            path_or_key,
        });
    };

    for raw in stderr.lines() {
        let (ts, line) = line_timestamp(raw.trim());

        if line.contains("trace:loaddll:") {
            if let Some(name) = extract_module_name(line) {
                let path = extract_quoted(line).unwrap_or_else(|| name.clone());
                loaded.insert(name);
                push_event(
                    ts,
                    &mut first_ts,
                    TraceEventKind::Dll,
                    "Loaded".to_owned(),
                    path,
                );
            }
            continue;
        }

        if line.contains("trace:reg:") {
            if let Some(op) = extract_channel_op(line, "trace:reg:") {
                registry.insert(op.clone());
                let key = extract_quoted(line).unwrap_or_default();
                push_event(ts, &mut first_ts, TraceEventKind::Registry, op, key);
            }
            continue;
        }

        if line.contains("trace:file:") {
            if let Some(op) = extract_channel_op(line, "trace:file:") {
                filesystem.insert(op.clone());
                let path = extract_quoted(line).unwrap_or_default();
                push_event(ts, &mut first_ts, TraceEventKind::Filesystem, op, path);
            }
            continue;
        }

        if let Some(after) = line.split_once("Call ").map(|(_, r)| r) {
            if let Some(call) = extract_relay_call(after) {
                calls.insert(call.clone());
                push_event(ts, &mut first_ts, TraceEventKind::Call, call, String::new());
            }
            continue;
        }

        let unimplemented = contains_ignore_ascii_case(line, "not implemented")
            || contains_ignore_ascii_case(line, "unimplemented")
            || contains_ignore_ascii_case(line, "no implementation for");
        if unimplemented && let Some(sym) = extract_unimplemented(line) {
            unsupported.insert(sym.clone());
            push_event(
                ts,
                &mut first_ts,
                TraceEventKind::Unsupported,
                sym,
                String::new(),
            );
        }
    }

    ParsedTrace {
        loaded: loaded.into_iter().collect(),
        calls: calls.into_iter().collect(),
        registry: registry.into_iter().collect(),
        filesystem: filesystem.into_iter().collect(),
        unsupported: unsupported.into_iter().collect(),
        timeline,
        timeline_truncated,
    }
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return n.is_empty();
    }
    h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn extract_channel_op(line: &str, tag: &str) -> Option<String> {
    let after = line.split_once(tag).map(|(_, r)| r)?.trim_start();
    let op: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if op.is_empty() { None } else { Some(op) }
}

fn extract_module_name(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let rest = &line[start + 1..];
    let end = rest.find('"')?;
    let path = &rest[..end];
    let base: String = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .chars()
        .take(TRACE_MAX_FIELD)
        .collect::<String>()
        .to_ascii_lowercase();
    if std::path::Path::new(&base)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
    {
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
        let p = parse_wine_trace(stderr, None);
        assert_eq!(p.loaded, vec!["kernel32.dll", "ntdll.dll"]);
        assert_eq!(p.timeline.len(), 2);
        assert_eq!(p.timeline[0].kind, TraceEventKind::Dll);
        assert!(p.timeline[0].path_or_key.ends_with("kernel32.dll"));
    }

    #[test]
    fn parses_relay_calls() {
        let stderr = "0024:Call kernel32.CreateFileW(0x1,0x2) ret=00401000\n\
                      0024:Call user32.MessageBoxA(0,\"hi\") ret=0";
        let p = parse_wine_trace(stderr, None);
        assert!(p.calls.contains(&"kernel32!CreateFileW".to_owned()));
        assert!(p.calls.contains(&"user32!MessageBoxA".to_owned()));
    }

    #[test]
    fn parses_unimplemented() {
        let stderr =
            "err:module:import_dll No implementation for dxgi.dll.SomeNewFn imported from ...";
        let p = parse_wine_trace(stderr, None);
        assert_eq!(p.unsupported, vec!["dxgi!SomeNewFn"]);
    }

    #[test]
    fn categorizes_registry_and_filesystem_access() {
        let stderr = "0024:trace:reg:RegOpenKeyExW (HKLM,...)\n\
                      0024:trace:reg:RegQueryValueExW (...)\n\
                      0024:trace:file:CreateFileW L\"C:\\\\x\"\n\
                      0024:trace:file:CreateFileW L\"C:\\\\y\"";
        let p = parse_wine_trace(stderr, None);
        assert_eq!(p.registry, vec!["RegOpenKeyExW", "RegQueryValueExW"]);
        assert_eq!(p.filesystem, vec!["CreateFileW"]);
        assert_eq!(p.timeline.len(), 4);
    }

    #[test]
    fn ignores_unrelated_output() {
        let p = parse_wine_trace("just some program stdout\nnothing to see", None);
        assert!(p.loaded.is_empty() && p.calls.is_empty() && p.unsupported.is_empty());
        assert!(p.timeline.is_empty());
    }

    #[test]
    fn timeline_uses_relative_timestamps() {
        let stderr = "184520.100:0024:trace:file:CreateFileW L\"C:\\\\first\"\n\
                      184520.350:0024:trace:file:ReadFile L\"C:\\\\second\"";
        let p = parse_wine_trace(stderr, None);
        assert_eq!(p.timeline.len(), 2);
        assert_eq!(p.timeline[0].at_ms, 0);
        assert_eq!(p.timeline[1].at_ms, 250);
    }

    #[test]
    fn lines_without_timestamps_still_produce_events() {
        let stderr = "0024:trace:file:CreateFileW L\"C:\\\\x\"";
        let p = parse_wine_trace(stderr, None);
        assert_eq!(p.timeline.len(), 1);
        assert_eq!(p.timeline[0].at_ms, 0);
    }

    #[test]
    fn filter_narrows_timeline_but_not_dedup_sets() {
        let stderr = "0024:trace:file:CreateFileW L\"C:\\\\app\\\\data.ini\"\n\
                      0024:trace:file:ReadFile L\"C:\\\\other\\\\thing.txt\"\n\
                      0024:trace:reg:RegOpenKeyExW L\"Software\\\\App\"";
        let p = parse_wine_trace(stderr, Some("*data.ini"));
        assert_eq!(p.timeline.len(), 1);
        assert_eq!(p.timeline[0].verb, "CreateFileW");
        assert_eq!(p.filesystem, vec!["CreateFileW", "ReadFile"]);
        assert_eq!(p.registry, vec!["RegOpenKeyExW"]);
    }

    #[test]
    fn filter_matches_verbs_too() {
        let stderr = "0024:trace:reg:RegOpenKeyExW L\"Software\"\n\
                      0024:trace:reg:RegCloseKey (1)";
        let p = parse_wine_trace(stderr, Some("RegOpen*"));
        assert_eq!(p.timeline.len(), 1);
    }

    #[test]
    fn glob_matcher_covers_star_and_question() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.dll", "KERNEL32.DLL"));
        assert!(glob_match("C:\\*\\data.???", "c:\\app\\data.ini"));
        assert!(glob_match("reg?pen*", "RegOpenKeyExW"));
        assert!(!glob_match("*.dll", "app.exe"));
        assert!(!glob_match("abc", "abcd"));
        assert!(glob_match("", ""));
        assert!(!glob_match("", "x"));
    }

    #[test]
    fn line_timestamp_parses_prefix_and_leaves_rest() {
        let (ts, rest) = line_timestamp("184520.856:0024:trace:file:CreateFileW");
        assert_eq!(ts, Some(184_520_856));
        assert_eq!(rest, "0024:trace:file:CreateFileW");
        let (ts, rest) = line_timestamp("0024:trace:file:CreateFileW");
        assert_eq!(ts, None);
        assert_eq!(rest, "0024:trace:file:CreateFileW");
    }
}
