use std::process::ExitCode;

use crate::cli::{CiCmd, CiProvider, ConfigCmd, Format};
use crate::{color, cwd, project};

pub(crate) fn config(op: &ConfigCmd, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        ConfigCmd::Check => {
            let p = project()?;
            let findings = lsw_core::configops::check(&p.root)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&findings).expect("serializes")
                );
            } else if findings.is_empty() {
                println!("lsw.toml: no problems found");
            } else {
                for f in &findings {
                    let tag = match f.severity {
                        lsw_core::configops::Severity::Warn => color::yellow("warning"),
                        lsw_core::configops::Severity::Error => color::red("error"),
                    };
                    println!("{tag}: {}", f.message);
                }
            }
            let has_error = findings
                .iter()
                .any(|f| f.severity == lsw_core::configops::Severity::Error);
            Ok(if has_error {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

pub(crate) fn ci(op: &CiCmd) -> lsw_core::Result<ExitCode> {
    match op {
        CiCmd::Init { provider } => {
            let CiProvider::Github = provider;
            let path = lsw_core::ciops::init_github(&cwd())?;
            println!("wrote {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
    }
}
