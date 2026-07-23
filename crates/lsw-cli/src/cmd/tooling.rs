use std::path::PathBuf;
use std::process::ExitCode;

use clap::CommandFactory;
use lsw_core::{Dirs, Status};

use crate::cli::{Cli, Format};
use crate::install::{default_prefix, run_install, write_man_page};
use crate::{active_env, color, project};

pub(crate) fn watch(dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    lsw_core::watchops::watch(&p, &env)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn doctor(dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let p = project().ok();
    let report = lsw_core::doctor(dirs, p.as_ref())?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("report serializes")
        );
    } else {
        println!("LSW Environment Doctor\n");
        for section in &report.sections {
            println!("{}", section.name);
            for r in &section.rows {
                let status = match r.status {
                    Status::Ok => color::green("OK"),
                    Status::Warn => color::yellow("WARNING"),
                    Status::Fail => color::red("FAIL"),
                };
                println!("  {:<18} {:<52} {}", r.label, r.value, status);
            }
            println!();
        }
        println!(
            "Overall: {}",
            if report.healthy {
                "healthy"
            } else {
                "PROBLEMS FOUND"
            }
        );
    }
    Ok(if report.healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

pub(crate) fn completions(shell: &clap_complete::Shell) -> lsw_core::Result<ExitCode> {
    let mut cmd = Cli::command();
    clap_complete::generate(*shell, &mut cmd, "lsw", &mut std::io::stdout());
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn install(prefix: &Option<PathBuf>) -> lsw_core::Result<ExitCode> {
    let prefix = prefix.clone().unwrap_or_else(default_prefix);
    run_install(&prefix)?;
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn man(dir: &Option<PathBuf>) -> lsw_core::Result<ExitCode> {
    let cmd = Cli::command();
    match dir {
        None => {
            clap_mangen::Man::new(cmd)
                .render(&mut std::io::stdout())
                .map_err(|e| lsw_core::Error::io(PathBuf::from("<stdout>"), e))?;
        }
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| lsw_core::Error::io(dir.clone(), e))?;
            write_man_page(&cmd, dir, "lsw")?;
            for sub in cmd.get_subcommands() {
                let name = format!("lsw-{}", sub.get_name());
                write_man_page(sub, dir, &name)?;
            }
            println!("wrote man pages to {}", dir.display());
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn explain(code: &str, format: Format) -> lsw_core::Result<ExitCode> {
    match lsw_core::explainops::explain(code) {
        Some(e) => {
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::json!({ "code": e.code, "summary": e.summary, "hint": e.hint })
                );
            } else {
                println!("{}  {}", e.code, e.summary);
                println!("  fix: {}", e.hint);
            }
            Ok(ExitCode::SUCCESS)
        }
        None => Ok(crate::usage_failure(
            format,
            &format!("no explanation for '{code}' (try an LSW#### code from an error message)"),
        )),
    }
}
