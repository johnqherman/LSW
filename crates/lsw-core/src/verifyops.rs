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
    let plan = plan(project, &build.artifacts, &remote_dir);

    run_ssh_plan(&host, &plan)
}

fn run_ssh_plan(host: &str, plan: &AgentPlan) -> Result<VerifyReport> {
    let mkdir = Command::new("ssh")
        .args(ssh_opts())
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
            .args(ssh_opts())
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
        let remote_cmd = format!(
            "cmd /c \"cd /d \"{}\" && \"{}\"\"",
            plan.remote_dir, program
        );
        let out = Command::new("ssh")
            .args(ssh_opts())
            .arg(host)
            .arg(&remote_cmd)
            .output()
            .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
        let code = out.status.code();
        if code != Some(0) {
            all_passed = false;
        }
        results.push(AgentResult {
            artifact: program.clone(),
            exit_code: code,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
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

fn ssh_opts() -> [&'static str; 4] {
    ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]
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
