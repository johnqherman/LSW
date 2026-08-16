use std::collections::HashSet;
use std::fs;
use std::path::Path;

use lsw_pe::{PeImage, PeInfo};

use crate::envops::Environment;
use crate::error::Result;

#[derive(Debug)]
/// Inspect Report.
pub struct InspectReport {
    /// Info.
    pub info: PeInfo,
    /// Details.
    pub details: lsw_pe::PeDetails,
    /// Hardening.
    pub hardening: lsw_pe::Hardening,
    /// Resources.
    pub resources: lsw_pe::Resources,
    /// Imports.
    pub imports: Vec<ImportStatus>,
}

#[derive(Debug)]
/// Import Status.
pub struct ImportStatus {
    /// Dll.
    pub dll: String,
    /// Available.
    pub available: Option<bool>,
}

/// Inspect.
pub fn inspect(path: &Path, env: Option<&Environment>) -> Result<InspectReport> {
    let image = PeImage::open(path)?;
    let info = image.info()?;

    let system32 = env.map(system32_names);
    let imports = image
        .imports()?
        .into_iter()
        .map(|dll| {
            let available = system32.as_ref().map(|names| dll_available(names, &dll));
            ImportStatus { dll, available }
        })
        .collect();

    let details = image.details()?;
    let hardening = image.hardening()?;
    let resources = image.resources().unwrap_or_default();
    Ok(InspectReport {
        info,
        details,
        hardening,
        resources,
        imports,
    })
}

fn system32_names(env: &Environment) -> HashSet<String> {
    let system32 = env.layout.drive_c().join("windows/system32");
    let mut names = HashSet::new();
    if let Ok(entries) = fs::read_dir(&system32) {
        for entry in entries.flatten().take(1_000_000) {
            names.insert(entry.file_name().to_string_lossy().to_ascii_lowercase());
        }
    }
    names
}

fn dll_available(system32: &HashSet<String>, dll: &str) -> bool {
    let wanted = dll.to_ascii_lowercase();
    if wanted.starts_with("api-ms-win-") || wanted.starts_with("ext-ms-win-") {
        return true;
    }
    system32.contains(&wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_system32() -> HashSet<String> {
        ["kernel32.dll", "ntdll.dll", "user32.dll"]
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn dll_available_case_insensitive() {
        let names = sample_system32();
        assert!(dll_available(&names, "KERNEL32.DLL"));
        assert!(dll_available(&names, "Kernel32.dll"));
    }

    #[test]
    fn dll_available_api_set_shortcut() {
        let names = HashSet::new();
        assert!(dll_available(&names, "api-ms-win-crt-runtime-l1-1-0.dll"));
        assert!(dll_available(&names, "ext-ms-win-ntuser-gui-l1-3-0.dll"));
    }

    #[test]
    fn dll_available_missing() {
        let names = sample_system32();
        assert!(!dll_available(&names, "nonexistent.dll"));
    }

    #[test]
    fn import_status_debug() {
        let s = ImportStatus {
            dll: "foo.dll".into(),
            available: Some(true),
        };
        let dbg = format!("{s:?}");
        assert!(dbg.contains("foo.dll"));
    }

    #[test]
    fn inspect_report_fields() {
        let r = InspectReport {
            info: lsw_pe::PeInfo {
                format: lsw_pe::PeFormat::Pe32Plus,
                machine: lsw_pe::Machine::X86_64,
                subsystem: lsw_pe::Subsystem::Console,
            },
            details: lsw_pe::PeDetails {
                entry_point: 0x1000,
                image_base: 0x0014_0000_0000,
                sections: vec![],
            },
            hardening: lsw_pe::Hardening {
                aslr: true,
                high_entropy_va: Some(true),
                dep: true,
                cfg: false,
                seh: None,
                force_integrity: false,
                signed: false,
            },
            resources: lsw_pe::Resources::default(),
            imports: vec![ImportStatus {
                dll: "KERNEL32.dll".into(),
                available: Some(true),
            }],
        };
        assert_eq!(r.info.format, lsw_pe::PeFormat::Pe32Plus);
        assert_eq!(r.imports.len(), 1);
    }
}
