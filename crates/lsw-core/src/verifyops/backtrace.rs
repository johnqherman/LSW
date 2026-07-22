use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::project::Project;

use super::{
    default_remote_dir, expand_tilde, ssh_opts, validate_windows_dir, validate_windows_name, which,
};

const CDB_X64: &[&str] = &[
    "C:\\Program Files (x86)\\Windows Kits\\10\\Debuggers\\x64\\cdb.exe",
    "C:\\Program Files\\Windows Kits\\10\\Debuggers\\x64\\cdb.exe",
];
const CDB_X86: &[&str] = &[
    "C:\\Program Files (x86)\\Windows Kits\\10\\Debuggers\\x86\\cdb.exe",
    "C:\\Program Files\\Windows Kits\\10\\Debuggers\\x86\\cdb.exe",
];

#[derive(Debug, Serialize)]
pub struct NativeFrame {
    pub index: usize,
    pub call_site: String,
}

#[derive(Debug, Serialize)]
pub struct NativeBacktrace {
    pub host: String,
    pub exception: Option<String>,
    pub frames: Vec<NativeFrame>,
}

pub fn native_backtrace(
    project: &Project,
    program: &std::path::Path,
) -> Result<Option<NativeBacktrace>> {
    let cfg = &project.manifest.verify;
    let Some(host) = cfg.host.clone() else {
        return Ok(None);
    };
    let transport = cfg.transport.as_deref().unwrap_or("ssh");
    if transport != "ssh" {
        return Err(Error::UnsupportedTransport {
            transport: transport.to_owned(),
        });
    }
    if which("ssh").is_none() {
        return Err(Error::ToolMissing {
            tool: "ssh".into(),
            fix: "install openssh-client to reach the Windows verification host".into(),
        });
    }
    let identity = cfg.identity_file.as_deref().map(expand_tilde);
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    validate_windows_name(&name)?;

    let want_x86 = matches!(
        lsw_pe::detect(program),
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

    let mkdir = Command::new("ssh")
        .args(ssh_opts(identity.as_deref()))
        .arg(&host)
        .arg(format!(
            "cmd /c \"if not exist \"{remote_dir}\" mkdir \"{remote_dir}\"\""
        ))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    if !mkdir.status.success() {
        return Err(Error::ProbeFailed {
            host,
            detail: String::from_utf8_lossy(&mkdir.stderr).trim().to_owned(),
        });
    }
    let scp = Command::new("scp")
        .args(ssh_opts(identity.as_deref()))
        .arg(program)
        .arg(format!("{host}:{remote_fwd}/{name}"))
        .output()
        .map_err(|e| Error::io(PathBuf::from("scp"), e))?;
    if !scp.status.success() {
        return Err(Error::ProbeFailed {
            host,
            detail: String::from_utf8_lossy(&scp.stderr).trim().to_owned(),
        });
    }

    let out = Command::new("ssh")
        .args(ssh_opts(identity.as_deref()))
        .arg(&host)
        .arg(format!(
            "cmd /c \"\"{cdb}\" -c \"sxe av; g; kn 100; q\" \"{remote_target}\"\""
        ))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(Some(parse_backtrace(host, &stdout)))
}

fn detect_cdb(host: &str, identity: Option<&str>, paths: &[&str]) -> Result<Option<String>> {
    let checks = paths
        .iter()
        .enumerate()
        .map(|(i, p)| format!("if exist \"{p}\" echo LSWCDB{i}"))
        .collect::<Vec<_>>()
        .join(" & ");
    let out = Command::new("ssh")
        .args(ssh_opts(identity))
        .arg(host)
        .arg(format!("cmd /c \"{checks}\""))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    for (i, path) in paths.iter().enumerate() {
        if stdout.contains(&format!("LSWCDB{i}")) {
            return Ok(Some((*path).to_owned()));
        }
    }
    Ok(None)
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
            let text = text.rsplit_once("): ").map(|(_, t)| t).unwrap_or(text);
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
