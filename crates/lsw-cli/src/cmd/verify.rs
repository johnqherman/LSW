use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::active_env;
use crate::cli::Format;

pub(crate) fn verify(
    native_windows: &bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    if !*native_windows {
        eprintln!("note: only --native-windows verification is supported");
    }
    let report = lsw_core::verifyops::verify(&p, &env)?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("serializes")
        );
    } else {
        let status = match report.status {
            lsw_core::verifyops::VerifyStatus::WindowsVerified => "WINDOWS_VERIFIED",
            lsw_core::verifyops::VerifyStatus::WindowsFailed => "WINDOWS_FAILED",
            lsw_core::verifyops::VerifyStatus::WindowsUnavailable => "WINDOWS_UNAVAILABLE",
        };
        println!(
            "Native verification host: {}",
            report.host.as_deref().unwrap_or("none")
        );
        for r in &report.results {
            let reason = r
                .exit_code
                .and_then(lsw_core::verifyops::crash_reason)
                .map(|w| format!("  ({w})"))
                .unwrap_or_default();
            println!("  {:<24} exit {:?}{reason}", r.artifact, r.exit_code);
            if let Some(dump) = &r.dump {
                println!("      crash dump: {dump}");
                if let Ok(s) = lsw_core::dumpops::analyze(std::path::Path::new(dump)) {
                    match (&s.faulting_module, s.faulting_offset) {
                        (Some(m), Some(off)) => println!(
                            "      {} at {m}+{off:#x} (address {:#x})",
                            s.reason, s.crash_address
                        ),
                        _ => {
                            println!("      {} (address {:#x})", s.reason, s.crash_address)
                        }
                    }
                }
            }
        }
        println!("Status: {status}");
        println!("{}", report.detail);
    }
    Ok(match report.status {
        lsw_core::verifyops::VerifyStatus::WindowsVerified => ExitCode::SUCCESS,
        lsw_core::verifyops::VerifyStatus::WindowsUnavailable => ExitCode::FAILURE,
        lsw_core::verifyops::VerifyStatus::WindowsFailed => ExitCode::FAILURE,
    })
}

pub(crate) fn compat(
    program: &PathBuf,
    args: &[String],
    db: &bool,
    native: &bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let mut report = if *db {
        lsw_core::compatops::compat_recording(&env, program, args, dirs)?
    } else {
        lsw_core::compatops::compat(&env, program, args)?
    };
    let native = if *native {
        if let Some(probe) = lsw_core::verifyops::probe_imports(&p, program)? {
            lsw_core::compatops::apply_native(&mut report, &probe);
        }
        Some(lsw_core::verifyops::run_on_host(
            &p,
            std::slice::from_ref(program),
            args,
        )?)
    } else {
        None
    };
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({ "compat": report, "native": native })
            )
            .expect("serializes")
        );
    } else {
        println!("\nLSW Compatibility Report  {}\n", program.display());
        println!("Required imported DLLs:  {}", report.imported_dlls);
        println!("Imported API functions:  {}", report.imported_functions);
        println!("Loaded at runtime:       {}", report.loaded_dlls);
        println!("Supported locally:       {}", report.supported_locally);
        println!(
            "Potentially unsupported: {}",
            report.potentially_unsupported.len()
        );
        for d in &report.potentially_unsupported {
            println!("  ? {d}");
        }
        if !report.unsupported_apis.is_empty() {
            println!("Unimplemented APIs:");
            for a in &report.unsupported_apis {
                println!("  X {a}");
            }
        }
        let sup = |s: lsw_core::compatops::Support| match s {
            lsw_core::compatops::Support::Yes => "yes",
            lsw_core::compatops::Support::Partial => "partial",
            lsw_core::compatops::Support::No => "no",
            lsw_core::compatops::Support::Unused => "-",
        };
        let has_native = report.capabilities.iter().any(|c| c.native.is_some());
        if has_native {
            println!("\nFeature                Local      Native");
            println!("------------------------------------------");
            for c in &report.capabilities {
                let n = c.native.map(sup).unwrap_or("?");
                println!("{:<22} {:<10} {}", c.feature, sup(c.local), n);
            }
        } else {
            println!("\nFeature                Local");
            println!("---------------------------------");
            for c in &report.capabilities {
                println!("{:<22} {}", c.feature, sup(c.local));
            }
        }
        println!("\n{}", report.note);
        if let Some(nat) = &native {
            println!(
                "\nNative (real Windows): {:?} on {}",
                nat.status,
                nat.host.as_deref().unwrap_or("-")
            );
            for r in &nat.results {
                println!("  {:<24} exit {:?}", r.artifact, r.exit_code);
            }
            if nat.results.is_empty() {
                println!("  {}", nat.detail);
            }
        }
    }
    match &native {
        Some(nat)
            if !matches!(
                nat.status,
                lsw_core::verifyops::VerifyStatus::WindowsVerified
            ) =>
        {
            Ok(ExitCode::FAILURE)
        }
        _ => Ok(ExitCode::SUCCESS),
    }
}

pub(crate) fn compat_query(key: &str, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let db = lsw_core::compatdb::CompatDb::load(dirs)?;
    match db.query(key) {
        Some(entry) => {
            let verdict = match entry.verdict() {
                lsw_core::compatdb::Verdict::Supported => "SUPPORTED",
                lsw_core::compatdb::Verdict::Unsupported => "UNSUPPORTED",
            };
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(entry).expect("serializes")
                );
            } else {
                println!("{key}: {verdict}");
                println!(
                    "  observed supported {} / unsupported {} (last: {})",
                    entry.supported_count, entry.unsupported_count, entry.last_runtime
                );
            }
        }
        None => {
            if format == Format::Json {
                println!("{}", serde_json::json!({ "key": key, "found": false }));
            } else {
                println!(
                    "{key}: not in the compatibility database yet (run `lsw compat --db <app.exe>`)"
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn trace(
    program: &Path,
    args: &[String],
    relay: &bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (_p, env) = active_env(dirs)?;
    let report = lsw_core::traceops::trace(
        &env,
        program,
        args,
        &lsw_core::traceops::TraceOptions { relay: *relay },
    )?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("serializes")
        );
    } else {
        println!("\nLSW TRACE  {}\n", program.display());
        println!("Imported DLLs: {}", report.imported_dlls.len());
        println!("Loaded DLLs:   {}", report.loaded_dlls.len());
        for d in &report.loaded_dlls {
            println!("  + {d}");
        }
        if !report.observed_calls.is_empty() {
            println!("Observed API calls: {}", report.observed_calls.len());
            for c in &report.observed_calls {
                println!("  + {c}");
            }
        }
        if !report.registry_access.is_empty() {
            println!("Registry operations: {}", report.registry_access.len());
            for r in &report.registry_access {
                println!("  R {r}");
            }
        }
        if !report.filesystem_access.is_empty() {
            println!("Filesystem operations: {}", report.filesystem_access.len());
            for f in &report.filesystem_access {
                println!("  F {f}");
            }
        }
        if report.unsupported.is_empty() {
            println!("Unsupported APIs: none observed");
        } else {
            println!("Unsupported APIs:");
            for u in &report.unsupported {
                println!("  X {u}");
            }
        }
    }
    Ok(match report.exit_code {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(code.clamp(0, 255) as u8),
        None => ExitCode::FAILURE,
    })
}
