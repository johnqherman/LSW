use std::path::PathBuf;
use std::process::ExitCode;

use lsw_core::{BuildOptions, Dirs};

use crate::cli::{DomainFlags, Format, display_from, sandbox_from};
use crate::{active_env, exit_from_status, note_runtime_domain};

use super::{Picked, pick_built};

pub(crate) fn build(
    system: &Option<String>,
    update_lock: &bool,
    reproducible: &bool,
    aot: &bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let report = lsw_core::build(
        &p,
        &env,
        &BuildOptions {
            system: system.clone(),
            update_lock: *update_lock,
            reproducible: *reproducible,
            aot: *aot,
        },
    )?;
    if format == Format::Json {
        let artifacts: Vec<String> = report
            .artifacts
            .iter()
            .map(|a| a.display().to_string())
            .collect();
        let payload = serde_json::json!({
            "system": format!("{:?}", report.system),
            "commands": report.commands,
            "artifacts": artifacts,
            "lock_written": report.lock_written,
        });
        println!("{payload}");
    } else {
        println!("Build OK ({:?})", report.system);
        for a in &report.artifacts {
            println!("  {}", a.display());
        }
        if report.lock_written {
            println!("  wrote lsw.lock");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run(
    program: &Option<PathBuf>,
    args: &[String],
    domain: &DomainFlags,
    dump_on_crash: &bool,
    dirs: &Dirs,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let program = match program {
        Some(program) => program.clone(),
        None => {
            let build = lsw_core::build(&p, &env, &BuildOptions::default())?;
            match pick_built(&build, true) {
                Picked::None => {
                    eprintln!("the build produced no .exe to run; pass a program explicitly");
                    return Ok(ExitCode::FAILURE);
                }
                Picked::One(only) => {
                    println!("Running {}", only.display());
                    only
                }
                Picked::Many(many) => {
                    eprintln!("the build produced multiple executables; pick one:");
                    for exe in &many {
                        eprintln!("  lsw run {}", exe.display());
                    }
                    return Ok(ExitCode::FAILURE);
                }
            }
        }
    };
    let report = lsw_core::run(
        &env,
        Some(&p),
        &program,
        args,
        domain.domain(),
        sandbox_from(domain.sandbox),
        display_from(domain.headless),
    )?;
    crate::note_crash(&report.status);
    note_runtime_domain(&report);
    if *dump_on_crash && !report.status.success() {
        capture_crash_dump(&env, &program, args);
    }
    Ok(exit_from_status(report.status))
}

fn capture_crash_dump(env: &lsw_core::Environment, program: &std::path::Path, args: &[String]) {
    let dump = lsw_core::dumpops::dump_path_for(program);
    eprintln!("[lsw] re-running under winedbg to capture a crash dump");
    match lsw_core::dumpops::capture_wine_dump(env, program, args, &dump, false) {
        Ok(true) => {
            eprintln!("[lsw] crash dump written to {}", dump.display());
            match lsw_core::dumpops::analyze(&dump) {
                Ok(s) => {
                    eprintln!("[lsw] exception: {} at {:#x}", s.reason, s.crash_address);
                    if let (Some(m), Some(off)) = (&s.faulting_module, s.faulting_offset) {
                        eprintln!("[lsw] faulting:  {m}+{off:#x}");
                    }
                }
                Err(e) => eprintln!("[lsw] dump written but not decodable: {e}"),
            }
        }
        Ok(false) => {
            eprintln!("[lsw] no dump produced (the crash did not reproduce under winedbg)")
        }
        Err(e) => eprintln!("[lsw] dump capture failed: {e}"),
    }
}

pub(crate) fn exec(
    domain: &DomainFlags,
    command: &[String],
    dirs: &Dirs,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let (program, args) = command.split_first().expect("clap enforces non-empty");
    let report = lsw_core::run(
        &env,
        Some(&p),
        &PathBuf::from(program),
        args,
        domain.domain(),
        sandbox_from(domain.sandbox),
        display_from(domain.headless),
    )?;
    crate::note_crash(&report.status);
    note_runtime_domain(&report);
    Ok(exit_from_status(report.status))
}

pub(crate) fn test(headless: &bool, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let report = lsw_core::test(
        &p,
        &env,
        &lsw_core::TestOptions {
            headless: *headless,
        },
    )?;
    if format == Format::Json {
        crate::cmd::emit_json(&report);
    } else {
        let outcome = |o: lsw_core::Outcome| match o {
            lsw_core::Outcome::Pass => "PASS",
            lsw_core::Outcome::Fail => "FAIL",
            lsw_core::Outcome::NotRun => "NOT RUN",
        };
        println!("\nLSW Test Report\n");
        println!("Build:");
        println!(
            "  {:<24} {}",
            report.build.label,
            outcome(report.build.outcome)
        );
        println!("Runtime:");
        println!(
            "  {:<24} {}",
            report.runtime.label,
            outcome(report.runtime.outcome)
        );
        println!("Native:");
        println!(
            "  {:<24} {}",
            report.native.label,
            outcome(report.native.outcome)
        );
        if let (Some(p), Some(f)) = (report.tests_passed, report.tests_failed) {
            println!("\nTests:\n  {p} passed, {f} failed");
        }
        let compat = match report.compatibility {
            lsw_core::CompatStatus::LocalCompatibilityVerified => "LOCAL_COMPATIBILITY_VERIFIED",
            lsw_core::CompatStatus::LocalCompatibilityFailed => "LOCAL_COMPATIBILITY_FAILED",
            lsw_core::CompatStatus::NotRun => "NOT_RUN",
        };
        println!("\nCompatibility status:\n  {compat}");
    }
    crate::cmd::exit_ok(report.compatibility == lsw_core::CompatStatus::LocalCompatibilityVerified)
}

pub(crate) fn shell(windows: &bool, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    if *windows {
        println!(
            "Entering Windows shell (env: {}); 'exit' or double ctrl+c to leave.",
            env.name
        );
    } else {
        println!("Entering LSW shell (env: {}); 'exit' to leave.", env.name);
    }
    let status = lsw_core::shell(&env, Some(&p), *windows)?;
    if std::os::unix::process::ExitStatusExt::signal(&status).is_some() {
        println!();
        return Ok(ExitCode::SUCCESS);
    }
    Ok(exit_from_status(status))
}
