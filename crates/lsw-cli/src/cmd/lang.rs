use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::{DotnetCmd, Format, RustCmd, SdkCmd};
use crate::{active_env, cwd};

fn init_result(
    format: Format,
    language: &str,
    root: &std::path::Path,
    created: &[std::path::PathBuf],
) -> lsw_core::Result<ExitCode> {
    if format == Format::Json {
        let files: Vec<String> = created.iter().map(|f| f.display().to_string()).collect();
        crate::cmd::emit_json(
            &serde_json::json!({ "root": root.display().to_string(), "created": files }),
        );
    } else {
        println!("Initialized LSW {language} project at {}", root.display());
        for f in created {
            println!("  created {}", f.display());
        }
        println!("Next: lsw env create <name> && lsw build");
    }
    Ok(ExitCode::SUCCESS)
}

fn doctor_rows(title: &str, rows: &[(&str, &'static str)]) {
    println!("{title}\n");
    for (label, mark) in rows {
        println!("  {label:<18}{mark}");
    }
}

pub(crate) fn rust(op: &RustCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        RustCmd::Init { name } => {
            let report = lsw_core::rustops::init(&cwd()?, name.as_deref())?;
            init_result(format, "Rust", &report.root, &report.created)
        }

        RustCmd::Doctor => {
            let (_p, env) = active_env(dirs)?;
            let report = lsw_core::rustops::doctor(&env)?;
            if format == Format::Json {
                crate::cmd::emit_json(&report);
            } else {
                let mark = |c: lsw_core::rustops::Check| match c {
                    lsw_core::rustops::Check::Ok => "OK",
                    lsw_core::rustops::Check::NotConfigured => "NOT CONFIGURED",
                    lsw_core::rustops::Check::Missing => "MISSING",
                };
                doctor_rows(
                    &format!("LSW Rust Doctor  (target {})", report.target),
                    &[
                        ("Compiler target", mark(report.compiler_target)),
                        ("Linker", mark(report.linker)),
                        ("CRT", mark(report.crt)),
                        ("Windows imports", mark(report.windows_imports)),
                        ("Runtime execution", mark(report.runtime_execution)),
                        ("Native validation", mark(report.native_validation)),
                    ],
                );
            }
            let healthy = [
                report.compiler_target,
                report.linker,
                report.crt,
                report.windows_imports,
                report.runtime_execution,
            ]
            .iter()
            .all(|c| *c == lsw_core::rustops::Check::Ok);
            crate::cmd::exit_ok(healthy)
        }
    }
}

pub(crate) fn dotnet(op: &DotnetCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        DotnetCmd::Init { name } => {
            let report = lsw_core::dotnetops::init(&cwd()?, name.as_deref())?;
            init_result(format, "C#", &report.root, &report.created)
        }

        DotnetCmd::Doctor => {
            let (_p, env) = active_env(dirs)?;
            let report = lsw_core::dotnetops::doctor(&env)?;
            if format == Format::Json {
                crate::cmd::emit_json(&report);
            } else {
                let mark = |c: lsw_core::dotnetops::Check| match c {
                    lsw_core::dotnetops::Check::Ok => "OK",
                    lsw_core::dotnetops::Check::NotConfigured => "NOT CONFIGURED",
                    lsw_core::dotnetops::Check::Missing => "MISSING",
                };
                doctor_rows(
                    &format!("LSW C# Doctor  (RID {})", report.target),
                    &[
                        (".NET SDK", mark(report.sdk)),
                        ("Runtime ID", mark(report.runtime_identifier)),
                        ("Self-contained", mark(report.self_contained)),
                        ("Runtime execution", mark(report.runtime_execution)),
                        ("NativeAOT", mark(report.native_aot)),
                        ("Native validation", mark(report.native_validation)),
                    ],
                );
            }
            let healthy = [
                report.sdk,
                report.runtime_identifier,
                report.self_contained,
                report.runtime_execution,
            ]
            .iter()
            .all(|c| *c == lsw_core::dotnetops::Check::Ok);
            crate::cmd::exit_ok(healthy)
        }
    }
}

pub(crate) fn sdk(op: &SdkCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let json = format == Format::Json;
    match op {
        SdkCmd::Import { name, from, force } => {
            if !json {
                println!("Importing SDK '{name}' from {}...", from.display());
                println!(
                    "Note: you are responsible for the license terms of any Microsoft SDK content you import."
                );
            }
            let report = lsw_core::sdkops::import(dirs, name, from, *force)?;
            if json {
                crate::cmd::emit_json(&serde_json::json!({
                    "name": report.name,
                    "files_copied": report.files_copied,
                    "root": report.root.display().to_string(),
                }));
            } else {
                println!(
                    "Imported '{}' ({} files) to {}",
                    report.name,
                    report.files_copied,
                    report.root.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        SdkCmd::List => {
            let sdks = lsw_core::sdkops::list(dirs)?;
            if json {
                let items: Vec<_> = sdks
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "usable": s.usable,
                            "source": s.source.display().to_string(),
                        })
                    })
                    .collect();
                crate::cmd::emit_json(&items);
                return Ok(ExitCode::SUCCESS);
            }
            if sdks.is_empty() {
                println!("No SDKs imported. Import one with: lsw sdk import <name> --from <path>");
            }
            for s in sdks {
                println!(
                    "{:<20} {:<10} {}",
                    s.name,
                    if s.usable { "usable" } else { "incomplete" },
                    s.source.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        SdkCmd::Remove { name } => {
            lsw_core::sdkops::remove(dirs, name)?;
            if json {
                crate::cmd::emit_json(&serde_json::json!({ "name": name, "removed": true }));
            } else {
                println!("Removed SDK '{name}'");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
