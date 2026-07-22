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

    let remote_dir = cfg
        .remote_dir
        .clone()
        .unwrap_or_else(|| default_remote_dir(project));

    validate_windows_dir(&remote_dir)?;
    let plan = plan(project, &build.artifacts, &remote_dir);
    for (_, name) in &plan.uploads {
        validate_windows_name(name)?;
    }

    let identity = cfg.identity_file.as_deref().map(expand_tilde);
    run_ssh_plan(&host, &plan, identity.as_deref())
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

fn validate_windows_dir(dir: &str) -> Result<()> {
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

fn validate_windows_name(name: &str) -> Result<()> {
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

fn run_ssh_plan(host: &str, plan: &AgentPlan, identity: Option<&str>) -> Result<VerifyReport> {
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
        let sentinel = "__LSW_EXIT__";
        let remote_cmd = format!(
            "cmd /c \"cd /d \"{dir}\" && \"{prog}\" & echo {sentinel}!errorlevel!\"",
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
        results.push(AgentResult {
            artifact: program.clone(),
            exit_code: remote_code,
            stdout,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
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

fn default_remote_dir(project: &Project) -> String {
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
