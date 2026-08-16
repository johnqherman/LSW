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
    const MAX_DIRS: usize = 100_000;
    const MAX_ENTRIES_PER_DIR: usize = 1_000_000;
    let mut stack = vec![root.to_path_buf()];
    let mut total = 0;
    let mut queued = 1usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names = Vec::new();
        for entry in entries.flatten().take(MAX_ENTRIES_PER_DIR) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            if is_dir && !SKIP.contains(&name.as_str()) && queued < MAX_DIRS {
                queued += 1;
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

fn vc_runtime_row(project: &Project, env: &envops::Environment) -> Option<Row> {
    let dynamic = project.manifest.toolchain.link == lsw_config::LinkMode::Dynamic;
    let msvc_abi = env.manifest.toolchain.provider == "clang-cl";
    if !dynamic || !msvc_abi {
        return None;
    }
    let found = crate::depsops::vc_runtime_dirs(&env.manifest.toolchain.sysroot);
    Some(if found.is_empty() {
        row(
            "VC++ runtime",
            "dynamic MSVC-ABI build but no vcruntime/msvcp DLLs in the SDK sysroot; \
             import the redist DLLs into your SDK splat (lsw sdk import) so \
             `lsw package --bundle-deps` can ship them, or use static linking (default)",
            Status::Warn,
        )
    } else {
        row(
            "VC++ runtime",
            format!(
                "redist DLLs found in the SDK sysroot ({} dir(s)); bundle with lsw package --bundle-deps",
                found.len()
            ),
            Status::Ok,
        )
    })
}

fn build_tool_rows(project: Option<&Project>) -> Vec<Row> {
    let mut checks: Vec<(String, bool)> = Vec::new();
    if let Some(p) = project {
        if let Some(build) = &p.manifest.build {
            if let Some(program) = build.command.first() {
                checks.push((program.clone(), true));
            }
        } else if let Some(system) = crate::buildops::detect_build_system(&p.root) {
            match system {
                crate::buildops::BuildSystem::Cmake => {
                    checks.push(("cmake".into(), true));
                    checks.push(("ninja".into(), false));
                }
                crate::buildops::BuildSystem::Cargo => checks.push(("cargo".into(), true)),
                crate::buildops::BuildSystem::Make => checks.push(("make".into(), true)),
                crate::buildops::BuildSystem::Ninja => checks.push(("ninja".into(), true)),
                crate::buildops::BuildSystem::Meson => {
                    checks.push(("meson".into(), true));
                    checks.push(("ninja".into(), true));
                }
                crate::buildops::BuildSystem::Zig => checks.push(("zig".into(), true)),
                crate::buildops::BuildSystem::Dotnet => checks.push(("dotnet".into(), true)),
                crate::buildops::BuildSystem::Explicit => {}
            }
        }
        if !p.manifest.dependencies.is_empty() {
            checks.push(("curl".into(), true));
            checks.push(("tar".into(), true));
        }
    } else {
        checks.push(("cmake".into(), false));
        checks.push(("ninja".into(), false));
    }
    if checks.is_empty() {
        return vec![row(
            "Build system",
            "none detected - scaffold one with lsw init, or set a [build] command",
            Status::Warn,
        )];
    }
    checks
        .into_iter()
        .map(
            |(program, required)| match crate::buildops::which(&program) {
                Some(path) => row(&program, path.display().to_string(), Status::Ok),
                None => row(
                    &program,
                    format!("'{program}' not found on PATH - install it with your package manager"),
                    if required { Status::Fail } else { Status::Warn },
                ),
            },
        )
        .collect()
}

fn target_support_rows(arch: lsw_config::TargetArch) -> Vec<Row> {
    let mut rows = Vec::new();
    let yes_no = |label: &str, supported: bool, detail: &str| {
        row(
            label,
            if supported {
                format!("supported ({detail})")
            } else {
                format!("not available for {arch} ({detail})")
            },
            if supported { Status::Ok } else { Status::Warn },
        )
    };
    rows.push(match arch.rust_gnu_triple() {
        Some(t) => yes_no("Rust", true, t),
        None => yes_no("Rust", false, "no GNU-ABI Windows target"),
    });
    rows.push(match crate::buildops::zig_target(arch) {
        Some(t) => yes_no("Zig", true, t),
        None => yes_no("Zig", false, "no zig cross target"),
    });
    rows.push(match crate::dotnetops::dotnet_rid(arch) {
        Some(r) => yes_no(".NET", true, r),
        None => yes_no(".NET", false, "no Windows RID"),
    });
    rows.push(match crate::depsops::repo_for(arch) {
        Ok((repo, _)) => yes_no("deps add (msys2)", true, repo),
        Err(_) => yes_no("deps add (msys2)", false, "no mingw package repository"),
    });
    rows
}

pub fn doctor(dirs: &Dirs, project: Option<&Project>) -> Result<DoctorReport> {
    let mut sections = Vec::new();

    let uname = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .map_or_else(
            || "unknown".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        );
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
            let major: Option<u32> = rt
                .version
                .split(['.', '-'])
                .next()
                .and_then(|v| v.parse().ok());
            let old = major.is_some_and(|m| m < 9);
            runtime_rows.push(row(
                "Wine",
                if old {
                    format!(
                        "{} ({}) - older than wine 9; expect missing APIs, upgrade if possible",
                        rt.version,
                        rt.executable.display()
                    )
                } else {
                    format!("{} ({})", rt.version, rt.executable.display())
                },
                if old { Status::Warn } else { Status::Ok },
            ));
        }
        Err(e) => runtime_rows.push(row("Wine", e.to_string(), Status::Fail)),
    }
    sections.push(Section {
        name: "Runtime".into(),
        rows: runtime_rows,
    });

    let arch = project.map_or(lsw_config::TargetArch::X86_64, |p| p.manifest.target.arch);
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

    sections.push(Section {
        name: "Build tools".into(),
        rows: build_tool_rows(project),
    });

    sections.push(Section {
        name: "Target support".into(),
        rows: target_support_rows(arch),
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
                if let Some(vc_row) = vc_runtime_row(p, &env) {
                    rows.push(vc_row);
                }
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

    let verify_row = match project.map(|p| &p.manifest.verify) {
        Some(v) if v.host.is_some() => {
            let host = v.host.as_deref().unwrap_or("");
            let transport = v.transport.as_deref().unwrap_or("ssh");
            if crate::verifyops::SUPPORTED_TRANSPORTS.contains(&transport) {
                row(
                    "Verification host",
                    format!("{host} (transport: {transport})"),
                    Status::Ok,
                )
            } else {
                row(
                    "Verification host",
                    format!(
                        "{host} (unsupported transport '{transport}'; use ssh, winrm, or https)"
                    ),
                    Status::Fail,
                )
            }
        }
        _ => row(
            "Verification host",
            "not configured (local compatibility results only)",
            Status::Warn,
        ),
    };
    sections.push(Section {
        name: "Native Windows".into(),
        rows: vec![verify_row],
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
