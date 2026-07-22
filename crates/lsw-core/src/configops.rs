use std::path::Path;

use lsw_config::ProjectManifest;
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warn,
    Error,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

const KNOWN_API: &[&str] = &[
    "winxp",
    "xp",
    "vista",
    "winvista",
    "win7",
    "windows7",
    "win8",
    "windows8",
    "win8.1",
    "win81",
    "win10",
    "windows10",
    "win11",
    "windows11",
];
const KNOWN_NETWORK: &[&str] = &["host", "isolated", "none"];
const KNOWN_REG_TYPE: &[&str] = &[
    "string",
    "dword",
    "expand",
    "reg_sz",
    "reg_dword",
    "reg_expand_sz",
];

pub fn lint(manifest: &ProjectManifest) -> Vec<Finding> {
    let mut out = Vec::new();
    let warn = |out: &mut Vec<Finding>, m: String| {
        out.push(Finding {
            severity: Severity::Warn,
            message: m,
        })
    };
    let error = |out: &mut Vec<Finding>, m: String| {
        out.push(Finding {
            severity: Severity::Error,
            message: m,
        })
    };

    if manifest.project.name.trim().is_empty() {
        error(&mut out, "[project] name is empty".into());
    }

    if let Some(api) = &manifest.target.api
        && !KNOWN_API.contains(&api.to_ascii_lowercase().as_str())
    {
        warn(
            &mut out,
            format!(
                "[target] api = \"{api}\" is unrecognized; no _WIN32_WINNT defines will be emitted"
            ),
        );
    }

    if !KNOWN_NETWORK.contains(&manifest.sandbox.network.as_str()) {
        error(
            &mut out,
            format!(
                "[sandbox] network = \"{}\" is invalid (expected host, isolated, or none)",
                manifest.sandbox.network
            ),
        );
    }

    for seed in &manifest.registry.seed {
        if !KNOWN_REG_TYPE.contains(&seed.kind.to_ascii_lowercase().as_str()) {
            warn(
                &mut out,
                format!(
                    "[[registry.seed]] type = \"{}\" is unrecognized; will be treated as REG_SZ",
                    seed.kind
                ),
            );
        }
        if !seed.key.contains('\\') {
            warn(
                &mut out,
                format!(
                    "[[registry.seed]] key \"{}\" does not look like a hive path",
                    seed.key
                ),
            );
        }
    }

    if manifest.verify.host.is_some()
        && let Some(transport) = &manifest.verify.transport
        && transport != "ssh"
    {
        error(
            &mut out,
            format!("[verify] transport = \"{transport}\" is unsupported (only ssh)"),
        );
    }

    out
}

pub fn check(project_root: &Path) -> Result<Vec<Finding>> {
    let manifest = ProjectManifest::load(&project_root.join(lsw_config::PROJECT_MANIFEST))?;
    Ok(lint(&manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_flags_bad_network_and_unknown_api() {
        let mut m = ProjectManifest::new("demo");
        m.target.api = Some("win99".into());
        m.sandbox.network = "wat".into();
        let findings = lint(&m);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Error && f.message.contains("network"))
        );
        assert!(
            findings
                .iter()
                .any(|f| f.severity == Severity::Warn && f.message.contains("api"))
        );
    }

    #[test]
    fn lint_clean_manifest_has_no_findings() {
        let m = ProjectManifest::new("demo");
        assert!(lint(&m).is_empty());
    }
}
