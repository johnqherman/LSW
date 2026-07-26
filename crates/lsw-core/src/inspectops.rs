use std::collections::HashSet;
use std::fs;
use std::path::Path;

use lsw_pe::{PeImage, PeInfo};

use crate::envops::Environment;
use crate::error::Result;

#[derive(Debug)]
pub struct InspectReport {
    pub info: PeInfo,
    pub details: lsw_pe::PeDetails,
    pub hardening: lsw_pe::Hardening,
    pub resources: lsw_pe::Resources,
    pub imports: Vec<ImportStatus>,
}

#[derive(Debug)]
pub struct ImportStatus {
    pub dll: String,
    pub available: Option<bool>,
}

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
