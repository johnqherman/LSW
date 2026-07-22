use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
pub struct AuditCheck {
    pub name: String,
    pub enabled: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct AuditReport {
    pub checks: Vec<AuditCheck>,
    pub hardened: bool,
}

fn check(name: &str, enabled: bool, detail: &str) -> AuditCheck {
    AuditCheck {
        name: name.to_owned(),
        enabled,
        detail: detail.to_owned(),
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
            h.aslr,
            "image can be loaded at a randomized base",
        ),
        check(
            "High-entropy ASLR",
            h.high_entropy_va,
            "64-bit address space randomization",
        ),
        check("DEP (NXCOMPAT)", h.dep, "data pages are non-executable"),
        check("Control Flow Guard", h.cfg, "indirect-call target checking"),
        check(
            "SafeSEH",
            h.seh,
            "structured exception handlers are validated",
        ),
        check(
            "Forced integrity",
            h.force_integrity,
            "signature is checked at load time",
        ),
        check(
            "Authenticode signed",
            h.signed,
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
