use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::buildops::{self, BuildOptions};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerifyStatus {
    WindowsVerified,
    WindowsFailed,
    WindowsUnavailable,
}

#[derive(Debug, Serialize)]
pub struct AgentResult {
    pub artifact: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dump: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VerifyReport {
    pub status: VerifyStatus,
    pub host: Option<String>,
    pub results: Vec<AgentResult>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentPlan {
    pub uploads: Vec<(PathBuf, String)>,
    pub run: Vec<String>,
    pub remote_dir: String,
}

pub fn plan(project: &Project, artifacts: &[PathBuf], remote_dir: &str) -> AgentPlan {
    let mut uploads = Vec::new();
    let mut run = Vec::new();
    for artifact in artifacts {
        let name = artifact
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        uploads.push((project.root.join(artifact), name.clone()));
        if artifact
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            run.push(name);
        }
    }
    AgentPlan {
        uploads,
        run,
        remote_dir: remote_dir.to_owned(),
    }
}

pub fn verify(project: &Project, env: &Environment) -> Result<VerifyReport> {
    let build = buildops::build(
        project,
        env,
        &BuildOptions {
            system: None,
            update_lock: false,
            reproducible: false,
        },
    )?;
    run_on_host(project, &build.artifacts)
}

pub fn run_on_host(project: &Project, artifacts: &[PathBuf]) -> Result<VerifyReport> {
    let cfg = &project.manifest.verify;
    let Some(host) = cfg.host.clone() else {
        return Ok(VerifyReport {
            status: VerifyStatus::WindowsUnavailable,
            host: None,
            results: Vec::new(),
            detail: "no [verify] host configured in lsw.toml; \
                     add transport = \"ssh\" and host = \"user@win-host\" to enable native verification"
                .into(),
        });
    };

    let transport = cfg.transport.as_deref().unwrap_or("ssh");
    match transport {
        "ssh" => {}
        "winrm" => return crate::winrmops::run_on_host(project, artifacts),
        other => {
            return Err(Error::UnsupportedTransport {
                transport: other.to_owned(),
            });
        }
    }
    if which("ssh").is_none() {
        return Err(Error::ToolMissing {
            tool: "ssh".into(),
            fix: "install openssh-client to reach the Windows verification host".into(),
        });
    }

    let remote_dir = cfg
        .remote_dir
        .clone()
        .unwrap_or_else(|| default_remote_dir(project));

    validate_windows_dir(&remote_dir)?;
    let plan = plan(project, artifacts, &remote_dir);
    for (_, name) in &plan.uploads {
        validate_windows_name(name)?;
    }

    let identity = cfg.identity_file.as_deref().map(expand_tilde);
    let dump_local = project.root.join("verify-dumps");
    run_ssh_plan(
        &host,
        &plan,
        identity.as_deref(),
        cfg.dump_dir.as_deref(),
        &dump_local,
    )
}

#[derive(Debug, Serialize)]
pub struct DllProbe {
    pub name: String,
    pub loaded: bool,
    pub missing_functions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportProbe {
    pub host: String,
    pub dlls: Vec<DllProbe>,
}

pub fn probe_imports(project: &Project, program: &std::path::Path) -> Result<Option<ImportProbe>> {
    let cfg = &project.manifest.verify;
    let Some(host) = cfg.host.clone() else {
        return Ok(None);
    };
    let imports = lsw_pe::imported_symbols(program).unwrap_or_default();
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

    let mut grouped: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (dll, func) in imports {
        let clean = func.trim();
        if clean.is_empty() || !clean.chars().all(|c| c.is_ascii_graphic() && c != '\'') {
            continue;
        }
        let entry = grouped.entry(dll.to_ascii_lowercase()).or_default();
        if entry.len() < 256 && !entry.iter().any(|f| f == clean) {
            entry.push(clean.to_owned());
        }
    }
    if grouped.is_empty() {
        return Ok(Some(ImportProbe {
            host,
            dlls: Vec::new(),
        }));
    }

    let identity = cfg.identity_file.as_deref().map(expand_tilde);
    let remote_dir = cfg
        .remote_dir
        .clone()
        .unwrap_or_else(|| default_remote_dir(project));
    validate_windows_dir(&remote_dir)?;
    let remote_fwd = remote_dir.replace('\\', "/");
    let remote_script = format!("{remote_dir}\\lsw_probe.ps1");
    let script = probe_script(&grouped);

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

    let local_script =
        std::env::temp_dir().join(format!("lsw_probe_{}.ps1", project.manifest.project.name));
    std::fs::write(&local_script, script.as_bytes())
        .map_err(|e| Error::io(local_script.clone(), e))?;
    let scp = Command::new("scp")
        .args(ssh_opts(identity.as_deref()))
        .arg(&local_script)
        .arg(format!("{host}:{remote_fwd}/lsw_probe.ps1"))
        .output()
        .map_err(|e| Error::io(PathBuf::from("scp"), e))?;
    let _ = std::fs::remove_file(&local_script);
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
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{remote_script}\""
        ))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(Error::ProbeFailed {
            host,
            detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut dlls: std::collections::BTreeMap<String, DllProbe> = grouped
        .keys()
        .map(|name| {
            (
                name.clone(),
                DllProbe {
                    name: name.clone(),
                    loaded: false,
                    missing_functions: Vec::new(),
                },
            )
        })
        .collect();
    for line in stdout.lines() {
        let cols: Vec<&str> = line.trim().split('\t').collect();
        match cols.as_slice() {
            ["DLL", name, status] => {
                if let Some(d) = dlls.get_mut(&name.to_ascii_lowercase()) {
                    d.loaded = *status == "OK";
                }
            }
            ["FN", name, func, "MISSING"] => {
                if let Some(d) = dlls.get_mut(&name.to_ascii_lowercase()) {
                    d.missing_functions.push((*func).to_owned());
                }
            }
            _ => {}
        }
    }

    Ok(Some(ImportProbe {
        host,
        dlls: dlls.into_values().collect(),
    }))
}

fn probe_script(grouped: &std::collections::BTreeMap<String, Vec<String>>) -> String {
    let mut s = String::from(
        "$ErrorActionPreference='SilentlyContinue'\n\
         Add-Type @\"\n\
         using System;\n\
         using System.Runtime.InteropServices;\n\
         public static class LswProbe {\n\
         [DllImport(\"kernel32\", SetLastError=true, CharSet=CharSet.Unicode)] public static extern IntPtr LoadLibraryW(string n);\n\
         [DllImport(\"kernel32\", SetLastError=true, CharSet=CharSet.Ansi)] public static extern IntPtr GetProcAddress(IntPtr h, string p);\n\
         }\n\
         \"@\n",
    );
    for (dll, funcs) in grouped {
        s.push_str(&format!("$h=[LswProbe]::LoadLibraryW('{dll}')\n"));
        s.push_str(&format!(
            "if($h -eq [IntPtr]::Zero){{Write-Output \"DLL`t{dll}`tMISSING\"}}else{{Write-Output \"DLL`t{dll}`tOK\"\n"
        ));
        for func in funcs {
            s.push_str(&format!(
                "if([LswProbe]::GetProcAddress($h,'{func}') -eq [IntPtr]::Zero){{Write-Output \"FN`t{dll}`t{func}`tMISSING\"}}\n"
            ));
        }
        s.push_str("}\n");
    }
    s
}

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
        if let Some(idx) = trimmed.find(" - code ") {
            if !trimmed.contains("80000003") {
                let text = trimmed[..idx].trim_end();
                let text = text.rsplit_once("): ").map(|(_, t)| t).unwrap_or(text);
                exception = Some(text.to_owned());
            }
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

pub fn crash_reason(exit_code: i32) -> Option<&'static str> {
    let status = exit_code as u32;
    let reason = match status {
        0xC0000005 => "access violation",
        0xC000001D => "illegal instruction",
        0xC0000094 => "integer divide by zero",
        0xC00000FD => "stack overflow",
        0xC0000409 => "stack buffer overrun (fail-fast)",
        0xC0000135 => "a required DLL was not found",
        0xC0000139 => "an entry point was not found in a DLL",
        0xC0000142 => "DLL initialization failed",
        0xC000007B => "invalid image format (wrong architecture?)",
        _ => return None,
    };
    Some(reason)
}

fn expand_tilde(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => format!("{}/{}", home.to_string_lossy(), rest),
            None => path.to_owned(),
        },
        None => path.to_owned(),
    }
}

pub(crate) fn validate_windows_dir(dir: &str) -> Result<()> {
    let bad = || Error::UnsafeRemotePath {
        value: dir.to_owned(),
    };
    let rest = dir
        .strip_prefix(|c: char| c.is_ascii_alphabetic())
        .and_then(|r| r.strip_prefix(':'))
        .and_then(|r| r.strip_prefix('\\'))
        .ok_or_else(bad)?;
    if rest.is_empty() {
        return Err(bad());
    }
    for segment in rest.split('\\') {
        if segment.is_empty() {
            return Err(bad());
        }
        validate_windows_name(segment).map_err(|_| bad())?;
    }
    Ok(())
}

pub(crate) fn validate_windows_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if ok {
        Ok(())
    } else {
        Err(Error::UnsafeRemotePath {
            value: name.to_owned(),
        })
    }
}

fn newest_dump(host: &str, identity: Option<&str>, dump_remote: &str, exe: &str) -> Option<String> {
    let out = Command::new("ssh")
        .args(ssh_opts(identity))
        .arg(host)
        .arg(format!("cmd /c dir /b /o-d \"{dump_remote}\\{exe}.*.dmp\""))
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let t = l.trim();
        (!t.is_empty() && t.to_ascii_lowercase().ends_with(".dmp")).then(|| t.to_owned())
    })
}

fn collect_dump(
    host: &str,
    identity: Option<&str>,
    dump_remote: &str,
    exe: &str,
    before: Option<&str>,
    dump_local: &std::path::Path,
) -> Option<String> {
    let mut name = None;
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        if let Some(found) = newest_dump(host, identity, dump_remote, exe) {
            if before != Some(found.as_str()) {
                name = Some(found);
                break;
            }
        }
    }
    let name = name?;
    std::fs::create_dir_all(dump_local).ok()?;
    let dest = dump_local.join(&name);
    let remote_fwd = dump_remote.replace('\\', "/");
    let scp = Command::new("scp")
        .args(ssh_opts(identity))
        .arg(format!("{host}:{remote_fwd}/{name}"))
        .arg(&dest)
        .output()
        .ok()?;
    scp.status.success().then(|| dest.display().to_string())
}

fn run_ssh_plan(
    host: &str,
    plan: &AgentPlan,
    identity: Option<&str>,
    dump_remote: Option<&str>,
    dump_local: &std::path::Path,
) -> Result<VerifyReport> {
    let mkdir = Command::new("ssh")
        .args(ssh_opts(identity))
        .arg(host)
        .arg(format!(
            "cmd /c \"if not exist \"{}\" mkdir \"{}\"\"",
            plan.remote_dir, plan.remote_dir
        ))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    if !mkdir.status.success() {
        return Ok(VerifyReport {
            status: VerifyStatus::WindowsUnavailable,
            host: Some(host.to_owned()),
            results: Vec::new(),
            detail: format!(
                "could not reach Windows host '{host}': {}",
                String::from_utf8_lossy(&mkdir.stderr).trim()
            ),
        });
    }

    for (local, remote_name) in &plan.uploads {
        let dest = format!("{host}:{}/{remote_name}", plan.remote_dir);
        let scp = Command::new("scp")
            .args(ssh_opts(identity))
            .arg(local)
            .arg(&dest)
            .output()
            .map_err(|e| Error::io(PathBuf::from("scp"), e))?;
        if !scp.status.success() {
            return Ok(VerifyReport {
                status: VerifyStatus::WindowsUnavailable,
                host: Some(host.to_owned()),
                results: Vec::new(),
                detail: format!(
                    "failed to upload {}: {}",
                    local.display(),
                    String::from_utf8_lossy(&scp.stderr).trim()
                ),
            });
        }
    }

    let mut results = Vec::new();
    let mut all_passed = true;
    for program in &plan.run {
        let dump_before = dump_remote.and_then(|dir| newest_dump(host, identity, dir, program));
        let sentinel = "__LSW_EXIT__";
        let remote_cmd = format!(
            "cmd /v:on /c \"cd /d \"{dir}\" && \"{prog}\" & echo {sentinel}!errorlevel!\"",
            dir = plan.remote_dir,
            prog = program,
        );
        let out = Command::new("ssh")
            .args(ssh_opts(identity))
            .arg(host)
            .arg(&remote_cmd)
            .output()
            .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();

        let remote_code = parse_sentinel_code(&stdout, sentinel);
        if remote_code.is_none() {
            return Ok(VerifyReport {
                status: VerifyStatus::WindowsUnavailable,
                host: Some(host.to_owned()),
                results,
                detail: format!(
                    "lost the Windows host '{host}' during the run phase (ssh exit {:?}): {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            });
        }
        if remote_code != Some(0) {
            all_passed = false;
        }
        let dump = match (dump_remote, remote_code) {
            (Some(dir), Some(code)) if crash_reason(code).is_some() => collect_dump(
                host,
                identity,
                dir,
                program,
                dump_before.as_deref(),
                dump_local,
            ),
            _ => None,
        };
        results.push(AgentResult {
            artifact: program.clone(),
            exit_code: remote_code,
            stdout,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            dump,
        });
    }

    let status = if results.is_empty() {
        VerifyStatus::WindowsUnavailable
    } else if all_passed {
        VerifyStatus::WindowsVerified
    } else {
        VerifyStatus::WindowsFailed
    };
    let detail = match status {
        VerifyStatus::WindowsVerified => "all artifacts ran successfully on native Windows".into(),
        VerifyStatus::WindowsFailed => "one or more artifacts failed on native Windows".into(),
        VerifyStatus::WindowsUnavailable => "no runnable artifacts were produced".into(),
    };
    Ok(VerifyReport {
        status,
        host: Some(host.to_owned()),
        results,
        detail,
    })
}

fn parse_sentinel_code(stdout: &str, sentinel: &str) -> Option<i32> {
    stdout
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix(sentinel))
        .and_then(|n| n.trim().parse::<i32>().ok())
}

fn ssh_opts(identity: Option<&str>) -> Vec<String> {
    let mut opts = vec![
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=10".to_owned(),
    ];
    if let Some(identity) = identity {
        opts.push("-o".to_owned());
        opts.push("IdentitiesOnly=yes".to_owned());
        opts.push("-i".to_owned());
        opts.push(identity.to_owned());
    }
    opts
}

pub(crate) fn default_remote_dir(project: &Project) -> String {
    format!("C:\\lsw-verify\\{}", project.manifest.project.name)
}

fn which(program: &str) -> Option<PathBuf> {
    buildops::which(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsw_config::ProjectManifest;

    fn project_with(root: &std::path::Path) -> Project {
        Project {
            root: root.to_path_buf(),
            manifest: ProjectManifest::new("demo"),
        }
    }

    #[test]
    fn crash_reason_decodes_common_ntstatus() {
        assert_eq!(crash_reason(-1073741819), Some("access violation"));
        assert_eq!(
            crash_reason(0xC0000135u32 as i32),
            Some("a required DLL was not found")
        );
        assert_eq!(crash_reason(0), None);
        assert_eq!(crash_reason(3), None);
    }

    #[test]
    fn ssh_opts_adds_identity_flags_only_when_set() {
        let base = ssh_opts(None);
        assert!(base.contains(&"BatchMode=yes".to_owned()));
        assert!(!base.iter().any(|o| o == "-i"));

        let withkey = ssh_opts(Some("/home/u/.ssh/lsw_verify"));
        let pos = withkey.iter().position(|o| o == "-i").unwrap();
        assert_eq!(withkey[pos + 1], "/home/u/.ssh/lsw_verify");
        assert!(withkey.contains(&"IdentitiesOnly=yes".to_owned()));
    }

    #[test]
    fn expand_tilde_uses_home() {
        unsafe { std::env::set_var("HOME", "/home/tester") };
        assert_eq!(
            expand_tilde("~/.ssh/lsw_verify"),
            "/home/tester/.ssh/lsw_verify"
        );
        assert_eq!(expand_tilde("/abs/key"), "/abs/key");
    }

    #[test]
    fn plan_uploads_all_and_runs_only_exes() {
        let tmp = tempfile::tempdir().unwrap();
        let project = project_with(tmp.path());
        let artifacts = vec![
            PathBuf::from("build/app.exe"),
            PathBuf::from("build/plugin.dll"),
        ];
        let p = plan(&project, &artifacts, "C:\\lsw-verify\\demo");
        assert_eq!(p.uploads.len(), 2);
        assert_eq!(p.run, vec!["app.exe"]);
        assert_eq!(p.remote_dir, "C:\\lsw-verify\\demo");
        assert_eq!(p.uploads[0].1, "app.exe");
    }

    #[test]
    fn windows_dir_validator_rejects_injection() {
        assert!(validate_windows_dir("C:\\lsw-verify\\demo").is_ok());
        assert!(validate_windows_dir("D:\\a\\b_c.1").is_ok());
        for bad in [
            "C:\\x\" & powershell -enc AA & rem \"",
            "C:\\x & del /q /s C:\\",
            "C:\\a\\..\\b",
            "C:\\a b",
            "C:\\a|b",
            "\\\\unc\\share",
            "relative\\path",
            "C:\\",
        ] {
            assert!(validate_windows_dir(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn windows_name_validator_rejects_metachars() {
        assert!(validate_windows_name("app.exe").is_ok());
        for bad in ["a&b.exe", "a b.exe", "..", "a\"b", "a\\b", ""] {
            assert!(validate_windows_name(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn sentinel_distinguishes_remote_code_from_ssh_failure() {
        assert_eq!(
            parse_sentinel_code("Hello\r\n__LSW_EXIT__0\r\n", "__LSW_EXIT__"),
            Some(0)
        );
        assert_eq!(
            parse_sentinel_code("boom\n__LSW_EXIT__3\n", "__LSW_EXIT__"),
            Some(3)
        );
        assert_eq!(
            parse_sentinel_code("ssh: connect timed out", "__LSW_EXIT__"),
            None
        );
    }

    #[test]
    fn verify_config_parses_from_manifest() {
        let manifest = "[project]\nname = \"demo\"\n\n\
                        [verify]\ntransport = \"ssh\"\nhost = \"me@winbox\"\n";
        let m: ProjectManifest = toml::from_str(manifest).unwrap();
        assert_eq!(m.verify.transport.as_deref(), Some("ssh"));
        assert_eq!(m.verify.host.as_deref(), Some("me@winbox"));
    }

    #[test]
    fn missing_verify_section_is_default() {
        let m: ProjectManifest = toml::from_str("[project]\nname = \"demo\"\n").unwrap();
        assert_eq!(m.verify, lsw_config::VerifySection::default());
        assert!(m.verify.host.is_none());
    }
}
