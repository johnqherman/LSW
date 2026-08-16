use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Audit Status.
pub enum AuditStatus {
    /// Enabled.
    Enabled,
    /// Disabled.
    Disabled,
    /// Not Applicable.
    NotApplicable,
}

impl AuditStatus {
    fn from_flag(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

#[derive(Debug, Serialize)]
/// Audit Check.
pub struct AuditCheck {
    /// Name.
    pub name: String,
    /// Status.
    pub status: AuditStatus,
    /// Detail.
    pub detail: String,
}

#[derive(Debug, Serialize)]
/// Audit Report.
pub struct AuditReport {
    /// Checks.
    pub checks: Vec<AuditCheck>,
    /// Hardened.
    pub hardened: bool,
}

fn check(name: &str, status: AuditStatus, detail: &str) -> AuditCheck {
    AuditCheck {
        name: name.to_owned(),
        status,
        detail: detail.to_owned(),
    }
}

fn tri(name: &str, value: Option<bool>, detail: &str, na_detail: &str) -> AuditCheck {
    match value {
        Some(v) => check(name, AuditStatus::from_flag(v), detail),
        None => check(name, AuditStatus::NotApplicable, na_detail),
    }
}

/// Audit.
pub fn audit(path: &Path) -> Result<AuditReport> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let h = lsw_pe::hardening(path)?;
    let checks = vec![
        check(
            "ASLR (DYNAMICBASE)",
            AuditStatus::from_flag(h.aslr),
            "image can be loaded at a randomized base",
        ),
        tri(
            "High-entropy ASLR",
            h.high_entropy_va,
            "64-bit address space randomization",
            "64-bit-only mitigation; this is a 32-bit image",
        ),
        check(
            "DEP (NXCOMPAT)",
            AuditStatus::from_flag(h.dep),
            "data pages are non-executable",
        ),
        check(
            "Control Flow Guard",
            AuditStatus::from_flag(h.cfg),
            "indirect-call target checking",
        ),
        tri(
            "SafeSEH",
            h.seh,
            "structured exception handlers are validated",
            "x86-only mitigation; x64 uses table-based exception handling",
        ),
        check(
            "Forced integrity",
            AuditStatus::from_flag(h.force_integrity),
            "signature is checked at load time",
        ),
        check(
            "Authenticode signed",
            AuditStatus::from_flag(h.signed),
            "embedded certificate table present",
        ),
    ];
    let hardened = h.aslr && h.dep;
    Ok(AuditReport { checks, hardened })
}

/// Exports.
pub fn exports(path: &Path) -> Result<Vec<String>> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    Ok(lsw_pe::exports(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_flag_enabled() {
        assert_eq!(AuditStatus::from_flag(true), AuditStatus::Enabled);
    }

    #[test]
    fn from_flag_disabled() {
        assert_eq!(AuditStatus::from_flag(false), AuditStatus::Disabled);
    }

    #[test]
    fn tri_some_true() {
        let c = tri("test", Some(true), "yes", "n/a");
        assert_eq!(c.status, AuditStatus::Enabled);
        assert_eq!(c.detail, "yes");
    }

    #[test]
    fn tri_some_false() {
        let c = tri("test", Some(false), "no", "n/a");
        assert_eq!(c.status, AuditStatus::Disabled);
    }

    #[test]
    fn tri_none() {
        let c = tri("test", None, "yes", "n/a");
        assert_eq!(c.status, AuditStatus::NotApplicable);
        assert_eq!(c.detail, "n/a");
    }

    #[test]
    fn check_builds_named_entry() {
        let c = check("ASLR", AuditStatus::Enabled, "ok");
        assert_eq!(c.name, "ASLR");
    }

    #[test]
    fn audit_rejects_nonexistent_file() {
        let r = audit(Path::new("/nonexistent/binary.exe"));
        assert!(r.is_err());
    }

    #[test]
    fn audit_status_serializes() {
        let json = serde_json::to_string(&AuditStatus::Enabled).unwrap();
        assert_eq!(json, "\"enabled\"");
        let json = serde_json::to_string(&AuditStatus::NotApplicable).unwrap();
        assert_eq!(json, "\"not_applicable\"");
    }
}
