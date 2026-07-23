use std::fs;
use std::process::Command;

use serde::Serialize;

use lsw_config::TargetArch;

use crate::envops::Environment;
use crate::error::{Error, Result};

fn cargo_package_name(raw: &str) -> String {
    let mut name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        name.push_str("app");
    }
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        name.insert(0, '_');
    }
    name
}

const TEMPLATE_CARGO: &str = r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
"#;

const TEMPLATE_MAIN: &str = r#"fn main() {
    println!("Hello from LSW (Rust)");
}
"#;

const TEMPLATE_LSW: &str = "";

#[derive(Debug)]
pub struct RustInitReport {
    pub root: std::path::PathBuf,
    pub created: Vec<std::path::PathBuf>,
}

pub fn init(parent: &std::path::Path, name: Option<&str>) -> Result<RustInitReport> {
    if let Some(n) = name {
        crate::envops::validate_name("project", n)?;
    }
    let (root, project_name) = match name {
        Some(n) => (parent.join(n), n.to_owned()),
        None => {
            let n = parent
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or_else(|| Error::InitFailed {
                    path: parent.to_path_buf(),
                    detail: "cannot derive a project name from this directory".into(),
                })?;
            (
                parent.to_path_buf(),
                crate::project::sanitize_project_name(&n),
            )
        }
    };

    if root.join("Cargo.toml").exists() || root.join(lsw_config::PROJECT_MANIFEST).exists() {
        return Err(Error::InitFailed {
            path: root,
            detail: "a Cargo.toml or lsw.toml already exists here".into(),
        });
    }

    fn write_file(
        root: &std::path::Path,
        rel: &str,
        contents: &str,
        created: &mut Vec<std::path::PathBuf>,
    ) -> Result<()> {
        use std::io::Write;
        let path = root.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io(dir.to_path_buf(), e))?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    Error::InitFailed {
                        path: path.clone(),
                        detail: format!("{rel} already exists; refusing to overwrite"),
                    }
                } else {
                    Error::io(path.clone(), e)
                }
            })?;
        created.push(path.clone());
        file.write_all(contents.as_bytes())
            .map_err(|e| Error::io(path.clone(), e))?;
        Ok(())
    }

    let mut created = Vec::new();
    let manifest_path = root.join(lsw_config::PROJECT_MANIFEST);
    let result: Result<()> = (|| {
        lsw_config::ProjectManifest::new(&project_name).save_new(&manifest_path)?;
        created.push(manifest_path.clone());
        write_file(
            &root,
            "Cargo.toml",
            &TEMPLATE_CARGO.replace("{name}", &cargo_package_name(&project_name)),
            &mut created,
        )?;
        write_file(&root, "src/main.rs", TEMPLATE_MAIN, &mut created)?;
        let _ = TEMPLATE_LSW;
        Ok(())
    })();

    match result {
        Ok(()) => Ok(RustInitReport { root, created }),
        Err(e) => {
            for path in created.iter().rev() {
                let _ = fs::remove_file(path);
            }
            Err(e)
        }
    }
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Ok,
    NotConfigured,
    Missing,
}

#[derive(Debug, Serialize)]
pub struct RustDoctor {
    pub target: String,
    pub compiler_target: Check,
    pub linker: Check,
    pub crt: Check,
    pub windows_imports: Check,
    pub runtime_execution: Check,
    pub native_validation: Check,
}

pub fn doctor(env: &Environment) -> Result<RustDoctor> {
    let arch = env.manifest.target_arch;
    let triple = arch.rust_gnu_triple();

    let cargo = which("cargo").is_some();
    let target_installed = triple.is_some_and(rust_target_installed);

    let linker_ok = env.manifest.toolchain.cc.is_file();
    let runtime_ok = env.manifest.runtime.executable.is_file();

    Ok(RustDoctor {
        target: triple.unwrap_or("<unsupported>").to_owned(),
        compiler_target: if cargo && target_installed {
            Check::Ok
        } else if triple.is_none() {
            Check::Missing
        } else {
            Check::NotConfigured
        },
        linker: bool_check(linker_ok),
        crt: bool_check(target_installed),
        windows_imports: bool_check(target_installed),
        runtime_execution: bool_check(runtime_ok),
        native_validation: Check::NotConfigured,
    })
}

fn bool_check(ok: bool) -> Check {
    if ok { Check::Ok } else { Check::NotConfigured }
}

pub fn ensure_target(arch: TargetArch) -> Result<()> {
    let triple = arch
        .rust_gnu_triple()
        .ok_or_else(|| Error::RustTargetUnavailable {
            arch: arch.to_string(),
        })?;
    if rust_target_installed(triple) {
        return Ok(());
    }
    if which("rustup").is_none() {
        return Err(Error::ToolMissing {
            tool: "rustup".into(),
            fix: format!("install rustup, or run: rustup target add {triple}"),
        });
    }
    let status = Command::new("rustup")
        .args(["target", "add", triple])
        .stdout(crate::diagnostic_stdio())
        .status()
        .map_err(|e| Error::io(std::path::PathBuf::from("rustup"), e))?;
    if !status.success() {
        return Err(Error::ToolMissing {
            tool: format!("rust target {triple}"),
            fix: format!("run: rustup target add {triple}"),
        });
    }
    Ok(())
}

fn rust_target_installed(triple: &str) -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim() == triple)
        })
        .unwrap_or(false)
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    crate::buildops::which(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_scaffolds_cargo_and_lsw_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let report = init(tmp.path(), Some("hello_rs")).unwrap();
        assert!(report.root.join("Cargo.toml").is_file());
        assert!(report.root.join("src/main.rs").is_file());
        assert!(report.root.join("lsw.toml").is_file());

        let (_, m) = lsw_config::ProjectManifest::discover(&report.root).unwrap();
        assert_eq!(m.project.name, "hello_rs");
        assert!(m.build.is_none());
    }

    #[test]
    fn init_refuses_over_existing_cargo() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), b"[package]").unwrap();
        assert!(init(tmp.path(), None).is_err());
    }
}
