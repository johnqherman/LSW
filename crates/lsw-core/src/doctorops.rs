use std::path::Path;

use serde::Serialize;

use lsw_config::Dirs;
use lsw_runtime::RuntimeProvider;

use crate::envops;
use crate::error::Result;
use crate::project::Project;

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub status: Status,
}

#[derive(Debug, Serialize)]
pub struct Section {
    pub name: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub sections: Vec<Section>,
    pub healthy: bool,
}

fn case_collisions(names: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for name in names {
        let lower = name.to_lowercase();
        match seen.get(&lower) {
            Some(first) => out.push(format!("{first} / {name}")),
            None => {
                seen.insert(lower, name.clone());
            }
        }
    }
    out
}

fn scan_case_collisions(root: &Path) -> usize {
    const SKIP: &[&str] = &["build", "target", ".git", "node_modules"];
    let mut stack = vec![root.to_path_buf()];
    let mut total = 0;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && !SKIP.contains(&name.as_str()) {
                stack.push(entry.path());
            }
            names.push(name);
        }
        total += case_collisions(&names).len();
    }
    total
}

fn row(label: &str, value: impl Into<String>, status: Status) -> Row {
    Row {
        label: label.to_owned(),
        value: value.into(),
        status,
    }
}

pub fn doctor(dirs: &Dirs, project: Option<&Project>) -> Result<DoctorReport> {
    let mut sections = Vec::new();

    let uname = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    sections.push(Section {
        name: "Host".into(),
        rows: vec![
            row("Linux kernel", &uname, Status::Ok),
            row("Architecture", std::env::consts::ARCH, Status::Ok),
        ],
    });

    let mut runtime_rows = Vec::new();
    match lsw_runtime::WineRuntime.resolve() {
        Ok(rt) => {
            runtime_rows.push(row(
                "Wine",
                format!("{} ({})", rt.version, rt.executable.display()),
                Status::Ok,
            ));
        }
        Err(e) => runtime_rows.push(row("Wine", e.to_string(), Status::Fail)),
    }
    sections.push(Section {
        name: "Runtime".into(),
        rows: runtime_rows,
    });

    let arch = project
        .map(|p| p.manifest.target.arch)
        .unwrap_or(lsw_config::TargetArch::X86_64);
    let mut tc_rows = Vec::new();
    for provider in lsw_toolchain::providers() {
        match provider.probe(arch) {
            Ok(report) if report.produced_pe => {
                tc_rows.push(row(provider.id(), "probe passed (produces PE)", Status::Ok));
            }
            Ok(report) => {
                tc_rows.push(row(
                    provider.id(),
                    format!("probe incomplete: {}", report.detail),
                    Status::Warn,
                ));
            }
            Err(e) => tc_rows.push(row(provider.id(), e.to_string(), Status::Warn)),
        }
    }
    if !tc_rows.iter().any(|r| r.status == Status::Ok) {
        tc_rows.push(row(
            "toolchain",
            "no provider can produce Windows binaries - install mingw-w64 or clang+lld",
            Status::Fail,
        ));
    }
    sections.push(Section {
        name: "Toolchain".into(),
        rows: tc_rows,
    });

    if let Some(p) = project {
        let mut rows = vec![row("lsw.toml", "valid", Status::Ok)];
        let collisions = scan_case_collisions(&p.root);
        rows.push(row(
            "Case sensitivity",
            if collisions == 0 {
                "no case-only filename collisions".to_owned()
            } else {
                format!(
                    "{collisions} case-only collision(s); may break on case-insensitive Windows"
                )
            },
            if collisions == 0 {
                Status::Ok
            } else {
                Status::Warn
            },
        ));
        match envops::resolve_active(dirs, p) {
            Ok(env) => {
                let diag = lsw_runtime::WineRuntime.diagnostics(&env.layout.prefix());
                rows.push(row("Environment", &env.name, Status::Ok));
                rows.push(row(
                    "Prefix",
                    if diag.prefix_initialized {
                        "initialized"
                    } else {
                        "not initialized - run lsw env create"
                    },
                    if diag.prefix_initialized {
                        Status::Ok
                    } else {
                        Status::Fail
                    },
                ));
                rows.push(row(
                    "Toolchain",
                    format!(
                        "{} {}",
                        env.manifest.toolchain.provider, env.manifest.toolchain.version
                    ),
                    Status::Ok,
                ));
            }
            Err(e) => rows.push(row("Environment", e.to_string(), Status::Fail)),
        }
        sections.push(Section {
            name: "Project".into(),
            rows,
        });
    }

    let sandbox_row = if lsw_runtime::find_bwrap().is_some() {
        row(
            "Strict sandbox",
            "available - run untrusted binaries with lsw run --sandbox strict",
            Status::Ok,
        )
    } else {
        row(
            "Strict sandbox",
            "bubblewrap not installed - only compatibility isolation is available",
            Status::Warn,
        )
    };
    sections.push(Section {
        name: "Security".into(),
        rows: vec![
            row(
                "Isolation model",
                "Wine prefix is a compatibility boundary, not a security boundary",
                Status::Ok,
            ),
            row(
                "Default host access",
                "Windows programs can reach the host filesystem via Z: unless sandboxed",
                Status::Ok,
            ),
            sandbox_row,
        ],
    });

    sections.push(Section {
        name: "Native Windows".into(),
        rows: vec![row(
            "Verification host",
            "not configured (local compatibility results only)",
            Status::Warn,
        )],
    });

    let healthy = !sections
        .iter()
        .flat_map(|s| &s.rows)
        .any(|r| r.status == Status::Fail);
    Ok(DoctorReport { sections, healthy })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_collisions_flags_case_only_duplicates() {
        let names = vec![
            "README.md".to_owned(),
            "readme.md".to_owned(),
            "src".to_owned(),
            "Main.rs".to_owned(),
        ];
        let hits = case_collisions(&names);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].to_lowercase().contains("readme.md"));
        assert!(case_collisions(&["a".to_owned(), "b".to_owned()]).is_empty());
    }
}
