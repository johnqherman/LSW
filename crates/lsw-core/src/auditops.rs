use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditStatus {
    Enabled,
    Disabled,
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
pub struct AuditCheck {
    pub name: String,
    pub status: AuditStatus,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct AuditReport {
    pub checks: Vec<AuditCheck>,
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

pub fn exports(path: &Path) -> Result<Vec<String>> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    Ok(lsw_pe::exports(path)?)
}
