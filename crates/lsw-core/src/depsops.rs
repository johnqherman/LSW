use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

const SYSTEM_DLLS: &[&str] = &[
    "kernel32.dll",
    "kernelbase.dll",
    "ntdll.dll",
    "user32.dll",
    "gdi32.dll",
    "advapi32.dll",
    "shell32.dll",
    "shlwapi.dll",
    "ole32.dll",
    "oleaut32.dll",
    "combase.dll",
    "comctl32.dll",
    "comdlg32.dll",
    "ws2_32.dll",
    "wininet.dll",
    "winhttp.dll",
    "crypt32.dll",
    "bcrypt.dll",
    "msvcrt.dll",
    "ucrtbase.dll",
    "rpcrt4.dll",
    "sechost.dll",
    "setupapi.dll",
    "version.dll",
    "winmm.dll",
];

fn is_system_dll(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("api-ms-win-")
        || lower.starts_with("ext-ms-win-")
        || SYSTEM_DLLS.contains(&lower.as_str())
}

fn resolve_dll(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let wanted = name.to_ascii_lowercase();
    for dir in dirs {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().to_ascii_lowercase() == wanted {
                return Some(entry.path());
            }
        }
    }
    None
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    Root,
    System,
    Resolved,
    Missing,
    Seen,
}

#[derive(Debug, Serialize)]
pub struct DepNode {
    pub name: String,
    pub kind: DepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub children: Vec<DepNode>,
}

fn search_dirs(env: Option<&Environment>, pe: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = pe.parent() {
        dirs.push(parent.to_path_buf());
    }
    if let Some(env) = env {
        dirs.push(env.manifest.toolchain.sysroot.join("bin"));
        dirs.push(env.layout.drive_c().join("windows/system32"));
    }
    dirs
}

fn build(name: &str, path: &Path, dirs: &[PathBuf], seen: &mut BTreeSet<String>) -> Vec<DepNode> {
    let mut children = Vec::new();
    let Ok(imports) = lsw_pe::imports(path) else {
        return children;
    };
    let mut names: Vec<String> = imports;
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by_key(|n| n.to_ascii_lowercase());
    for dep in names {
        if dep.eq_ignore_ascii_case(name) {
            continue;
        }
        children.push(node(&dep, dirs, seen));
    }
    children
}

fn node(name: &str, dirs: &[PathBuf], seen: &mut BTreeSet<String>) -> DepNode {
    let key = name.to_ascii_lowercase();
    if is_system_dll(name) {
        return DepNode {
            name: name.to_owned(),
            kind: DepKind::System,
            path: None,
            children: Vec::new(),
        };
    }
    match resolve_dll(name, dirs) {
        Some(resolved) => {
            if !seen.insert(key) {
                return DepNode {
                    name: name.to_owned(),
                    kind: DepKind::Seen,
                    path: Some(resolved.display().to_string()),
                    children: Vec::new(),
                };
            }
            let children = build(name, &resolved, dirs, seen);
            DepNode {
                name: name.to_owned(),
                kind: DepKind::Resolved,
                path: Some(resolved.display().to_string()),
                children,
            }
        }
        None => DepNode {
            name: name.to_owned(),
            kind: DepKind::Missing,
            path: None,
            children: Vec::new(),
        },
    }
}

pub fn tree(env: Option<&Environment>, pe: &Path) -> Result<DepNode> {
    if !pe.is_file() {
        return Err(Error::NotExecutable {
            program: pe.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    lsw_pe::detect(pe)?;
    let dirs = search_dirs(env, pe);
    let mut seen = BTreeSet::new();
    let name = pe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_owned());
    let children = build(&name, pe, &dirs, &mut seen);
    Ok(DepNode {
        name,
        kind: DepKind::Root,
        path: Some(pe.display().to_string()),
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_system_dll_matches_apisets_and_known_modules() {
        assert!(is_system_dll("KERNEL32.dll"));
        assert!(is_system_dll("api-ms-win-crt-runtime-l1-1-0.dll"));
        assert!(!is_system_dll("libstdc++-6.dll"));
    }

    #[test]
    fn resolve_dll_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("libFoo-1.dll"), b"x").unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        assert!(resolve_dll("libfoo-1.dll", &dirs).is_some());
        assert!(resolve_dll("missing.dll", &dirs).is_none());
    }
}
