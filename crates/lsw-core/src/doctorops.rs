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
            runtime_rows.push(row("Wine", format!("{} ({})", rt.version, rt.executable.display()), Status::Ok));
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
