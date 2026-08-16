use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::project::Project;

use super::{
    default_remote_dir, expand_tilde, ssh_opts, validate_windows_dir, validate_windows_name,
};

use crate::buildops::which;

const CDB_X64: &[&str] = &[
    "C:\\Program Files (x86)\\Windows Kits\\10\\Debuggers\\x64\\cdb.exe",
    "C:\\Program Files\\Windows Kits\\10\\Debuggers\\x64\\cdb.exe",
];
const CDB_X86: &[&str] = &[
    "C:\\Program Files (x86)\\Windows Kits\\10\\Debuggers\\x86\\cdb.exe",
    "C:\\Program Files\\Windows Kits\\10\\Debuggers\\x86\\cdb.exe",
];

#[derive(Debug, Serialize)]
/// Native Frame.
pub struct NativeFrame {
    /// Index.
    pub index: usize,
    /// Call site.
    pub call_site: String,
}

#[derive(Debug, Serialize)]
/// Native Backtrace.
pub struct NativeBacktrace {
    /// Host.
    pub host: String,
    /// Exception.
    pub exception: Option<String>,
    /// Frames.
    pub frames: Vec<NativeFrame>,
}

#[derive(Debug, Serialize)]
/// Native Analysis.
pub struct NativeAnalysis {
    /// Host.
    pub host: String,
    /// Bucket id.
    pub bucket_id: Option<String>,
    /// Failure class.
    pub failure_class: Option<String>,
    /// Symbol.
    pub symbol: Option<String>,
    /// Image.
    pub image: Option<String>,
    /// Frames.
    pub frames: Vec<NativeFrame>,
}

struct RemoteCdb {
    host: String,
    identity: Option<String>,
    cdb: String,
    remote_target: String,
    is_dump: bool,
}

fn prep_remote(project: &Project, target: &std::path::Path) -> Result<Option<RemoteCdb>> {
    let cfg = &project.manifest.verify;
    let Some(host) = cfg.host.clone() else {
        return Ok(None);
    };
    let transport = cfg.transport.as_deref().unwrap_or("ssh");
    if transport != "ssh" {
        return Ok(None);
    }
    super::validate_ssh_host(&host)?;
    if which("ssh").is_none() {
        return Err(Error::ToolMissing {
            tool: "ssh".into(),
            fix: "install openssh-client to reach the Windows verification host".into(),
        });
    }
    let identity = cfg.identity_file.as_deref().map(expand_tilde);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    validate_windows_name(&name)?;
    let is_dump = target
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dmp"));

    let want_x86 = matches!(
        lsw_pe::detect(target),
        Ok(lsw_pe::BinaryKind::Pe(info)) if info.machine == lsw_pe::Machine::X86
    );
    let paths = if want_x86 { CDB_X86 } else { CDB_X64 };
    let cdb = detect_cdb(&host, identity.as_deref(), paths)?.ok_or_else(|| Error::ToolMissing {
        tool: "cdb.exe".into(),
        fix: "install the Windows SDK Debugging Tools on the [verify] host".into(),
    })?;

    let remote_dir = cfg
        .remote_dir
        .clone()
        .unwrap_or_else(|| default_remote_dir(project));
    validate_windows_dir(&remote_dir)?;
    let remote_fwd = remote_dir.replace('\\', "/");
    let remote_target = format!("{remote_dir}\\{name}");

    if let Some(detail) = super::ensure_remote_dir(&host, identity.as_deref(), &remote_dir)? {
        return Err(Error::ProbeFailed { host, detail });
    }
    if let Some(detail) = super::scp_upload(
        &host,
        identity.as_deref(),
        target,
        &format!("{remote_fwd}/{name}"),
    )? {
        return Err(Error::ProbeFailed { host, detail });
    }
    Ok(Some(RemoteCdb {
        host,
        identity,
        cdb,
        remote_target,
        is_dump,
    }))
}

fn run_remote_cdb(remote: &RemoteCdb, script: &str) -> Result<String> {
    let invocation = if remote.is_dump {
        format!(
            "cmd /c \"\"{}\" -z \"{}\" -c \"{script}\"\"",
            remote.cdb, remote.remote_target
        )
    } else {
        format!(
            "cmd /c \"\"{}\" -c \"{script}\" \"{}\"\"",
            remote.cdb, remote.remote_target
        )
    };
    let out = super::ssh_command(&remote.host, remote.identity.as_deref(), &invocation)?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr);
        let detail = detail.trim();
        return Err(Error::ProbeFailed {
            host: remote.host.clone(),
            detail: if detail.is_empty() {
                format!("remote cdb exited with {}", out.status)
            } else {
                detail.to_owned()
            },
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Native backtrace.
pub fn native_backtrace(
    project: &Project,
    program: &std::path::Path,
) -> Result<Option<NativeBacktrace>> {
    let Some(remote) = prep_remote(project, program)? else {
        return Ok(None);
    };
    let stdout = run_remote_cdb(&remote, "sxe av; g; kn 100; q")?;
    Ok(Some(parse_backtrace(remote.host, &stdout)))
}

/// Native analyze.
pub fn native_analyze(
    project: &Project,
    target: &std::path::Path,
) -> Result<Option<NativeAnalysis>> {
    let Some(remote) = prep_remote(project, target)? else {
        return Ok(None);
    };
    let script = if remote.is_dump {
        "!analyze -v; q"
    } else {
        "sxe av; g; !analyze -v; q"
    };
    let stdout = run_remote_cdb(&remote, script)?;
    Ok(Some(parse_analysis(remote.host, &stdout)))
}

/// Native interactive.
pub fn native_interactive(
    project: &Project,
    target: &std::path::Path,
) -> Result<Option<std::process::ExitStatus>> {
    let Some(remote) = prep_remote(project, target)? else {
        return Ok(None);
    };
    let invocation = if remote.is_dump {
        format!("\"{}\" -z \"{}\"", remote.cdb, remote.remote_target)
    } else {
        format!("\"{}\" \"{}\"", remote.cdb, remote.remote_target)
    };
    let status = Command::new("ssh")
        .arg("-t")
        .args(ssh_opts(remote.identity.as_deref()))
        .arg(&remote.host)
        .arg(format!("cmd /c \"{invocation}\""))
        .status()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    Ok(Some(status))
}

fn detect_cdb(host: &str, identity: Option<&str>, paths: &[&str]) -> Result<Option<String>> {
    let checks = paths
        .iter()
        .enumerate()
        .map(|(i, p)| format!("if exist \"{p}\" echo LSWCDB{i}"))
        .collect::<Vec<_>>()
        .join(" & ");
    let out = super::ssh_command(host, identity, &format!("cmd /c \"{checks}\""))?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr);
        return Err(Error::ProbeFailed {
            host: host.to_owned(),
            detail: detail.trim().to_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for (i, path) in paths.iter().enumerate() {
        if stdout.contains(&format!("LSWCDB{i}")) {
            return Ok(Some((*path).to_owned()));
        }
    }
    Ok(None)
}

fn analysis_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.strip_prefix(':')?.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_owned())
    }
}

fn parse_analysis(host: String, stdout: &str) -> NativeAnalysis {
    let mut bucket_id = None;
    let mut failure_class = None;
    let mut symbol = None;
    let mut image = None;
    let mut frames = Vec::new();
    let mut in_stack = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("STACK_TEXT:") {
            in_stack = true;
            continue;
        }
        if in_stack {
            match trimmed.rsplit_once(" : ") {
                Some((_, site)) if !trimmed.is_empty() => {
                    frames.push(NativeFrame {
                        index: frames.len(),
                        call_site: site.trim().to_owned(),
                    });
                    continue;
                }
                _ => in_stack = false,
            }
        }
        bucket_id = bucket_id.or_else(|| analysis_value(trimmed, "FAILURE_BUCKET_ID"));
        failure_class = failure_class.or_else(|| analysis_value(trimmed, "EXCEPTION_CODE_STR"));
        symbol = symbol.or_else(|| analysis_value(trimmed, "SYMBOL_NAME"));
        image = image.or_else(|| analysis_value(trimmed, "IMAGE_NAME"));
    }
    NativeAnalysis {
        host,
        bucket_id,
        failure_class,
        symbol,
        image,
        frames,
    }
}

fn parse_backtrace(host: String, stdout: &str) -> NativeBacktrace {
    let mut exception = None;
    let mut frames = Vec::new();
    let mut in_stack = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find(" - code ")
            && !trimmed.contains("80000003")
        {
            let text = trimmed[..idx].trim_end();
            let text = text.rsplit_once("): ").map_or(text, |(_, t)| t);
            exception = Some(text.to_owned());
        }
        if trimmed.contains("Child") && trimmed.contains("RetAddr") {
            in_stack = true;
            continue;
        }
        if in_stack {
            let cols: Vec<&str> = trimmed.split_whitespace().collect();
            let is_frame = cols.len() >= 4
                && cols[0].len() <= 3
                && cols[0].chars().all(|c| c.is_ascii_hexdigit());
            if is_frame {
                if let Ok(index) = usize::from_str_radix(cols[0], 16) {
                    frames.push(NativeFrame {
                        index,
                        call_site: cols[3..].join(" "),
                    });
                }
            } else if !frames.is_empty() {
                in_stack = false;
            }
        }
    }
    NativeBacktrace {
        host,
        exception,
        frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_analysis_extracts_bucket_and_stack() {
        let stdout = "\
Some banner text\n\
EXCEPTION_CODE_STR:  c0000005\n\
FAILURE_BUCKET_ID:  INVALID_POINTER_READ_c0000005_app.exe!crash_me\n\
SYMBOL_NAME:  app!crash_me+12\n\
IMAGE_NAME:  app.exe\n\
STACK_TEXT:  \n\
00000049`2d2ff8a0 00007ff6`d9de1234 : 0000000000000000 : app!crash_me+0x12\n\
00000049`2d2ff8e0 00007ffc`aabbccdd : 0000000000000001 : app!main+0x34\n\
\n\
FOLLOWUP_NAME:  MachineOwner\n";
        let a = parse_analysis("winbox".into(), stdout);
        assert_eq!(
            a.bucket_id.as_deref(),
            Some("INVALID_POINTER_READ_c0000005_app.exe!crash_me")
        );
        assert_eq!(a.failure_class.as_deref(), Some("c0000005"));
        assert_eq!(a.symbol.as_deref(), Some("app!crash_me+12"));
        assert_eq!(a.image.as_deref(), Some("app.exe"));
        assert_eq!(a.frames.len(), 2);
        assert_eq!(a.frames[0].call_site, "app!crash_me+0x12");
        assert_eq!(a.frames[1].call_site, "app!main+0x34");
    }

    #[test]
    fn parse_analysis_handles_missing_fields() {
        let a = parse_analysis("winbox".into(), "no analyze output at all");
        assert!(a.bucket_id.is_none());
        assert!(a.frames.is_empty());
    }

    #[test]
    fn parse_backtrace_reads_exception_and_frames() {
        let stdout = "\
(1a2b.3c4d): Access violation - code c0000005 (first chance)\n\
 # Child-SP          RetAddr           Call Site\n\
Child             RetAddr\n\
00 00000049`2d2ff8a0 00007ff6`d9de1234 app!crash_me+0x12\n\
01 00000049`2d2ff8e0 00007ffc`aabbccdd app!main+0x34\n";
        let bt = parse_backtrace("winbox".into(), stdout);
        assert_eq!(bt.frames.len(), 2);
        assert_eq!(bt.frames[0].call_site, "app!crash_me+0x12");
    }
}
