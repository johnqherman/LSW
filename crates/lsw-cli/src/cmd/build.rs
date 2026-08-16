use std::path::PathBuf;
use std::process::ExitCode;

use lsw_core::{BuildOptions, Dirs};

use crate::cli::{BuildSystemArg, DomainFlags, Format, display_from, sandbox_from};
use crate::{active_env, exit_from_status, note_runtime_domain};

use super::{Picked, pick_built};

pub(crate) fn build(
    system: &Option<BuildSystemArg>,
    update_lock: &bool,
    reproducible: &bool,
    aot: &bool,
    all: bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    if all {
        return build_all(system, update_lock, reproducible, aot, dirs, format);
    }
    let (p, env) = active_env(dirs)?;
    let report = lsw_core::build(
        &p,
        &env,
        &BuildOptions {
            system: system.map(|s| s.as_str().to_owned()),
            update_lock: *update_lock,
            reproducible: *reproducible,
            aot: *aot,
            coverage: false,
        },
    )?;
    if format == Format::Json {
        let artifacts: Vec<String> = report
            .artifacts
            .iter()
            .map(|a| a.display().to_string())
            .collect();
        crate::cmd::emit_json(&serde_json::json!({
            "system": report.system.label(),
            "commands": report.commands,
            "artifacts": artifacts,
            "lock_written": report.lock_written,
        }));
    } else {
        println!("Build OK ({})", report.system.label());
        for a in &report.artifacts {
            println!("  {}", a.display());
        }
        if report.lock_written {
            println!("  wrote lsw.lock");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn build_all(
    system: &Option<BuildSystemArg>,
    update_lock: &bool,
    reproducible: &bool,
    aot: &bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let ws = lsw_core::Workspace::discover(&crate::cwd()?)?
        .ok_or(lsw_core::Error::NoBuildSystem)?;
    let env_name = crate::env_override();
    let mut all_ok = true;
    for member in &ws.members {
        let project = lsw_core::Project::discover(member)?;
        let env = match env_name {
            Some(name) => lsw_core::Environment::open(dirs, name)?,
            None => lsw_core::resolve_active(dirs, &project)?,
        };
        if format != Format::Json {
            println!("\n--- {} ---", member.display());
        }
        match lsw_core::build(
            &project,
            &env,
            &BuildOptions {
                system: system.map(|s| s.as_str().to_owned()),
                update_lock: *update_lock,
                reproducible: *reproducible,
                aot: *aot,
                coverage: false,
            },
        ) {
            Ok(report) => {
                if format == Format::Json {
                    crate::cmd::emit_json(&serde_json::json!({
                        "member": member.display().to_string(),
                        "system": report.system.label(),
                        "artifacts": report.artifacts.iter().map(|a| a.display().to_string()).collect::<Vec<_>>(),
                    }));
                } else {
                    println!("Build OK ({})", report.system.label());
                    for a in &report.artifacts {
                        println!("  {}", a.display());
                    }
                }
            }
            Err(e) => {
                all_ok = false;
                if format == Format::Json {
                    crate::cmd::emit_json(&serde_json::json!({
                        "member": member.display().to_string(),
                        "error": e.to_string(),
                    }));
                } else {
                    eprintln!("error: {e}");
                }
            }
        }
    }
    crate::cmd::exit_ok(all_ok)
}

pub(crate) fn run(
    program: &Option<PathBuf>,
    args: &[String],
    domain: &DomainFlags,
    dump_on_crash: &bool,
    dirs: &Dirs,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let program = if let Some(program) = program { program.clone() } else {
        let build = lsw_core::build(&p, &env, &BuildOptions::default())?;
        match pick_built(&build, true) {
            Picked::None => {
                eprintln!("the build produced no .exe to run; pass a program explicitly");
                return Ok(ExitCode::FAILURE);
            }
            Picked::One(only) => {
                eprintln!("Running {}", only.display());
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
            eprintln!("[lsw] no dump produced (the crash did not reproduce under winedbg)");
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

pub(crate) fn test(
    headless: &bool,
    junit: &Option<PathBuf>,
    coverage: bool,
    all: bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    if all {
        return test_all(headless, coverage, dirs, format);
    }
    let (p, env) = active_env(dirs)?;
    let report = lsw_core::test(
        &p,
        &env,
        &lsw_core::TestOptions {
            headless: *headless,
            junit: junit.clone(),
            coverage,
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

fn test_all(
    headless: &bool,
    coverage: bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let ws = lsw_core::Workspace::discover(&crate::cwd()?)?
        .ok_or(lsw_core::Error::NoTests)?;
    let env_name = crate::env_override();
    let mut all_ok = true;
    for member in &ws.members {
        let project = lsw_core::Project::discover(member)?;
        let env = match env_name {
            Some(name) => lsw_core::Environment::open(dirs, name)?,
            None => lsw_core::resolve_active(dirs, &project)?,
        };
        if format != Format::Json {
            println!("\n--- {} ---", member.display());
        }
        match lsw_core::test(
            &project,
            &env,
            &lsw_core::TestOptions {
                headless: *headless,
                junit: None,
                coverage,
            },
        ) {
            Ok(report) => {
                let passed = report.compatibility
                    == lsw_core::CompatStatus::LocalCompatibilityVerified;
                if !passed {
                    all_ok = false;
                }
                if format == Format::Json {
                    crate::cmd::emit_json(&serde_json::json!({
                        "member": member.display().to_string(),
                        "passed": report.tests_passed,
                        "failed": report.tests_failed,
                    }));
                } else if let (Some(p), Some(f)) = (report.tests_passed, report.tests_failed) {
                    println!("{p} passed, {f} failed");
                }
            }
            Err(e) => {
                all_ok = false;
                if format == Format::Json {
                    crate::cmd::emit_json(&serde_json::json!({
                        "member": member.display().to_string(),
                        "error": e.to_string(),
                    }));
                } else {
                    eprintln!("error: {e}");
                }
            }
        }
    }
    crate::cmd::exit_ok(all_ok)
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
