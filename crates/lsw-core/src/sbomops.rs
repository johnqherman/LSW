use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Error, Result};

fn build_sbom(
    app_name: &str,
    dlls: &[String],
    toolchain: Option<&str>,
    deps: &[(String, String, String)],
) -> Value {
    let mut components: Vec<Value> = dlls
        .iter()
        .map(|dll| {
            json!({
                "type": "library",
                "name": dll,
                "bom-ref": format!("dll:{dll}"),
                "scope": "required",
            })
        })
        .collect();
    for (name, version, sha256) in deps {
        let mut c = json!({
            "type": "library",
            "name": name,
            "version": version,
            "bom-ref": format!("dep:{name}"),
            "scope": "required",
        });
        if !sha256.is_empty() {
            c["hashes"] = json!([{ "alg": "SHA-256", "content": sha256 }]);
        }
        components.push(c);
    }
    if let Some(tc) = toolchain {
        components.push(json!({
            "type": "application",
            "name": tc,
            "bom-ref": format!("toolchain:{tc}"),
            "scope": "excluded",
        }));
    }
    json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "metadata": {
            "tools": [{ "vendor": "LSW", "name": "lsw" }],
            "component": {
                "type": "application",
                "name": app_name,
                "bom-ref": format!("app:{app_name}"),
            },
        },
        "components": components,
    })
}

pub fn sbom(path: &Path) -> Result<Value> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let mut dlls = lsw_pe::imports(path)?;
    dlls.sort();
    dlls.dedup();
    let name = path.file_name().map_or_else(
        || "artifact".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    let lock = crate::project::Project::discover(&std::env::current_dir().unwrap_or_default())
        .ok()
        .and_then(|p| lsw_config::Lockfile::load(&p.lockfile_path()).ok());
    let toolchain = lock
        .as_ref()
        .map(|lock| format!("{} {}", lock.toolchain.provider, lock.toolchain.version));
    let deps: Vec<(String, String, String)> = lock
        .map(|lock| {
            lock.dependencies
                .into_iter()
                .map(|(name, d)| (name, d.version, d.sha256))
                .collect()
        })
        .unwrap_or_default();

    Ok(build_sbom(&name, &dlls, toolchain.as_deref(), &deps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sbom_lists_dll_components_and_metadata() {
        let bom = build_sbom(
            "app.exe",
            &["KERNEL32.dll".to_owned(), "USER32.dll".to_owned()],
            Some("llvm-mingw 20250101"),
            &[("zlib".into(), "1.3.1-1".into(), "ab".repeat(32))],
        );
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["metadata"]["component"]["name"], "app.exe");
        let comps = bom["components"].as_array().unwrap();
        assert!(comps.iter().any(|c| c["name"] == "KERNEL32.dll"));
        assert!(comps.iter().any(|c| c["name"] == "llvm-mingw 20250101"));
        let zlib = comps.iter().find(|c| c["name"] == "zlib").unwrap();
        assert_eq!(zlib["version"], "1.3.1-1");
        assert_eq!(zlib["hashes"][0]["alg"], "SHA-256");
    }
}
