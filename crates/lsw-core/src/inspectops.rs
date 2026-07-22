use std::fs;
use std::path::Path;

use lsw_pe::{BinaryKind, PeInfo};

use crate::envops::Environment;
use crate::error::{Error, Result};

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
    let info = match lsw_pe::detect(path)? {
        BinaryKind::Pe(info) => info,
        other => {
            return Err(Error::NotExecutable {
                program: path.to_path_buf(),
                detail: format!("expected a PE binary, found {other:?}"),
            });
        }
    };

    let imports = lsw_pe::imports(path)?
        .into_iter()
        .map(|dll| {
            let available = env.map(|e| dll_available(e, &dll));
            ImportStatus { dll, available }
        })
        .collect();

    let details = lsw_pe::details(path)?;
    let hardening = lsw_pe::hardening(path)?;
    let resources = lsw_pe::resources(path).unwrap_or_default();
    Ok(InspectReport {
        info,
        details,
        hardening,
        resources,
        imports,
    })
}

fn dll_available(env: &Environment, dll: &str) -> bool {
    let wanted = dll.to_ascii_lowercase();
    if wanted.starts_with("api-ms-win-") || wanted.starts_with("ext-ms-win-") {
        return true;
    }
    let system32 = env.layout.drive_c().join("windows/system32");
    let Ok(entries) = fs::read_dir(&system32) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| e.file_name().to_string_lossy().to_ascii_lowercase() == wanted)
}
