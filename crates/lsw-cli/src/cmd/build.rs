use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lsw_core::{BuildOptions, Dirs};

use crate::cli::{Format, SandboxArg, display_from, domain_from_flags, sandbox_from};
use crate::{active_env, exit_from_status, note_runtime_domain};

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
        let payload = serde_json::json!({
            "system": format!("{:?}", report.system),
            "commands": report.commands,
            "artifacts": report.artifacts,
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
    program: &Path,
    args: &[String],
    host: &bool,
    windows: &bool,
    sandbox: &Option<SandboxArg>,
    headless: &bool,
    dirs: &Dirs,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let domain = domain_from_flags(*host, *windows);
    let report = lsw_core::run(
        &env,
        Some(&p),
        program,
        args,
        domain,
        sandbox_from(*sandbox),
        display_from(*headless),
    )?;
    note_runtime_domain(&report);
    Ok(exit_from_status(report.status))
}

pub(crate) fn exec(
    host: &bool,
    windows: &bool,
    sandbox: &Option<SandboxArg>,
    headless: &bool,
    command: &[String],
    dirs: &Dirs,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let domain = domain_from_flags(*host, *windows);
    let (program, args) = command.split_first().expect("clap enforces non-empty");
    let report = lsw_core::run(
        &env,
        Some(&p),
        &PathBuf::from(program),
        args,
        domain,
        sandbox_from(*sandbox),
        display_from(*headless),
    )?;
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
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("report serializes")
        );
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
    Ok(
        if report.compatibility == lsw_core::CompatStatus::LocalCompatibilityVerified {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        },
    )
}

pub(crate) fn shell(windows: &bool, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    if !*windows {
        println!("Entering LSW shell (env: {}); 'exit' to leave.", env.name);
    }
    let status = lsw_core::shell(&env, Some(&p), *windows)?;
    Ok(exit_from_status(status))
}
