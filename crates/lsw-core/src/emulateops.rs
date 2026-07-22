use std::path::PathBuf;

use lsw_config::TargetArch;
use lsw_runtime::Emulation;

use crate::error::{Error, Result};

fn family(arch: TargetArch) -> &'static str {
    match arch {
        TargetArch::X86_64 | TargetArch::X86 => "x86",
        TargetArch::Aarch64 | TargetArch::Armv7 | TargetArch::Arm64Ec => "arm",
    }
}

fn host_family() -> &'static str {
    match std::env::consts::ARCH {
        "x86" | "x86_64" => "x86",
        "arm" | "aarch64" => "arm",
        _ => "other",
    }
}

fn qemu_target(arch: TargetArch) -> (&'static str, &'static str) {
    match arch {
        TargetArch::X86_64 => ("qemu-x86_64", "LSW_WINE_X86_64"),
        TargetArch::X86 => ("qemu-i386", "LSW_WINE_X86"),
        TargetArch::Aarch64 | TargetArch::Arm64Ec => ("qemu-aarch64", "LSW_WINE_AARCH64"),
        TargetArch::Armv7 => ("qemu-arm", "LSW_WINE_ARM"),
    }
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn resolve(target: TargetArch) -> Result<Option<Emulation>> {
    if family(target) == host_family() {
        return Ok(None);
    }
    let (qemu_bin, wine_var) = qemu_target(target);
    let qemu = crate::buildops::which(qemu_bin).ok_or_else(|| Error::ToolMissing {
        tool: qemu_bin.to_owned(),
        fix: format!(
            "install qemu user-mode emulation ({qemu_bin}) to run {} binaries on {}",
            arch_label(target),
            std::env::consts::ARCH
        ),
    })?;
    let wine = std::env::var_os(wine_var)
        .map(PathBuf::from)
        .filter(|p| is_executable(p))
        .ok_or_else(|| Error::EmulationWineMissing {
            arch: arch_label(target).to_owned(),
            var: wine_var.to_owned(),
        })?;
    Ok(Some(Emulation { qemu, wine }))
}

fn arch_label(arch: TargetArch) -> &'static str {
    match arch {
        TargetArch::X86_64 => "x86_64",
        TargetArch::X86 => "x86",
        TargetArch::Aarch64 => "aarch64",
        TargetArch::Arm64Ec => "arm64ec",
        TargetArch::Armv7 => "armv7",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_family_needs_no_emulation() {
        if host_family() == "x86" {
            assert!(resolve(TargetArch::X86).unwrap().is_none());
            assert!(resolve(TargetArch::X86_64).unwrap().is_none());
        }
    }

    #[test]
    fn cross_family_without_wine_reports_missing() {
        if host_family() == "x86" {
            let err = resolve(TargetArch::Aarch64).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("LSW2011") || msg.contains("LSW2038"), "{msg}");
        }
    }

    #[test]
    fn qemu_binaries_map_by_arch() {
        assert_eq!(qemu_target(TargetArch::Aarch64).0, "qemu-aarch64");
        assert_eq!(qemu_target(TargetArch::Armv7).0, "qemu-arm");
        assert_eq!(qemu_target(TargetArch::X86).0, "qemu-i386");
    }
}
