use std::collections::BTreeMap;

use serde::Serialize;

use crate::envops::Environment;
use crate::error::Result;
use crate::traceops::{self, TraceOptions};

#[derive(Debug, Serialize)]
pub struct CompatReport {
    pub imported_dlls: usize,
    pub imported_functions: usize,
    pub loaded_dlls: usize,
    pub supported_locally: usize,
    pub potentially_unsupported: Vec<String>,
    pub unsupported_apis: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct Capability {
    pub feature: String,
    pub local: Support,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<Support>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    Yes,
    Partial,
    No,
    Unused,
}

struct Feature {
    name: &'static str,
    markers: &'static [&'static str],
    local: Support,
}

const FEATURES: &[Feature] = &[
    Feature {
        name: "Win32 console",
        markers: &["kernel32", "msvcrt", "ucrtbase", "api-ms-win-crt"],
        local: Support::Yes,
    },
    Feature {
        name: "Win32 GUI",
        markers: &["user32", "gdi32", "comctl32", "comdlg32"],
        local: Support::Yes,
    },
    Feature {
        name: "Direct3D",
        markers: &["d3d9", "d3d11", "d3d12", "dxgi"],
        local: Support::Partial,
    },
    Feature {
        name: "COM",
        markers: &["ole32", "oleaut32", "combase"],
        local: Support::Partial,
    },
    Feature {
        name: "Networking",
        markers: &["ws2_32", "wininet", "winhttp"],
        local: Support::Yes,
    },
    Feature {
        name: "Cryptography",
        markers: &["bcrypt", "crypt32", "ncrypt"],
        local: Support::Yes,
    },
    Feature {
        name: "Audio",
        markers: &["winmm", "dsound", "xaudio"],
        local: Support::Partial,
    },
];

pub fn compat(
    env: &Environment,
    program: &std::path::Path,
    args: &[String],
) -> Result<CompatReport> {
    compat_inner(env, program, args, None)
}

pub fn compat_recording(
    env: &Environment,
    program: &std::path::Path,
    args: &[String],
    dirs: &lsw_config::Dirs,
) -> Result<CompatReport> {
    compat_inner(env, program, args, Some(dirs))
}

fn compat_inner(
    env: &Environment,
    program: &std::path::Path,
    args: &[String],
    record_to: Option<&lsw_config::Dirs>,
) -> Result<CompatReport> {
    let trace = traceops::trace(env, program, args, &TraceOptions { relay: false })?;

    let imported: Vec<String> = trace
        .imported_dlls
        .iter()
        .map(|d| d.to_ascii_lowercase())
        .collect();
    let loaded: std::collections::BTreeSet<String> = trace
        .loaded_dlls
        .iter()
        .map(|d| d.to_ascii_lowercase())
        .collect();

    let mut supported = 0usize;
    let mut supported_keys = Vec::new();
    let mut unsupported = Vec::new();
    for dll in &imported {
        let is_apiset = dll.starts_with("api-ms-win-") || dll.starts_with("ext-ms-win-");
        if is_apiset || loaded.contains(dll) {
            supported += 1;
            supported_keys.push(dll.clone());
        } else {
            unsupported.push(dll.clone());
        }
    }

    if let Some(dirs) = record_to {
        let runtime = format!(
            "{} {}",
            env.manifest.runtime.provider, env.manifest.runtime.version
        );
        let mut fails = unsupported.clone();
        fails.extend(trace.unsupported.iter().cloned());
        let _guard = crate::compatdb::CompatDb::lock(dirs)?;
        let mut db = crate::compatdb::CompatDb::load(dirs)?;
        db.record(&runtime, &supported_keys, &fails);
        db.save(dirs)?;
    }

    let capabilities = classify(&imported);
    let imported_functions = lsw_pe::imported_symbols(program)
        .map(|s| s.len())
        .unwrap_or(0);

    Ok(CompatReport {
        imported_dlls: imported.len(),
        imported_functions,
        loaded_dlls: loaded.len(),
        supported_locally: supported,
        potentially_unsupported: unsupported,
        unsupported_apis: trace.unsupported,
        capabilities,
        note: "Local results reflect the compatibility runtime (Wine) only; \
               they do not guarantee identical behavior on native Windows."
            .to_owned(),
    })
}

fn classify(imported: &[String]) -> Vec<Capability> {
    let mut used: BTreeMap<&'static str, bool> = BTreeMap::new();
    for f in FEATURES {
        let is_used = imported
            .iter()
            .any(|dll| f.markers.iter().any(|m| dll.contains(m)));
        used.insert(f.name, is_used);
    }
    FEATURES
        .iter()
        .map(|f| Capability {
            feature: f.name.to_owned(),
            local: if used[f.name] {
                f.local
            } else {
                Support::Unused
            },
            native: None,
        })
        .collect()
}

pub fn apply_native(report: &mut CompatReport, probe: &crate::verifyops::ImportProbe) {
    let loaded: std::collections::BTreeMap<String, &crate::verifyops::DllProbe> = probe
        .dlls
        .iter()
        .map(|d| (d.name.to_ascii_lowercase(), d))
        .collect();
    for cap in &mut report.capabilities {
        let Some(feature) = FEATURES.iter().find(|f| f.name == cap.feature) else {
            continue;
        };
        let matched: Vec<&&crate::verifyops::DllProbe> = loaded
            .iter()
            .filter(|(name, _)| feature.markers.iter().any(|m| name.contains(m)))
            .map(|(_, d)| d)
            .collect();
        cap.native = Some(if matched.is_empty() {
            Support::Unused
        } else if matched
            .iter()
            .all(|d| d.loaded && d.missing_functions.is_empty())
        {
            Support::Yes
        } else if matched.iter().any(|d| d.loaded) {
            Support::Partial
        } else {
            Support::No
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_marks_used_and_unused_features() {
        let imports = vec!["kernel32.dll".to_owned(), "d3d11.dll".to_owned()];
        let caps = classify(&imports);
        let console = caps.iter().find(|c| c.feature == "Win32 console").unwrap();
        let d3d = caps.iter().find(|c| c.feature == "Direct3D").unwrap();
        let audio = caps.iter().find(|c| c.feature == "Audio").unwrap();
        assert_eq!(console.local, Support::Yes);
        assert_eq!(d3d.local, Support::Partial);
        assert_eq!(audio.local, Support::Unused);
    }
}
