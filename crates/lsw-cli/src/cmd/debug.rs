use std::path::Path;
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::Format;
use crate::{active_env, exit_from_status};

pub(crate) fn debug(
    program: &Path,
    args: &[String],
    gdb: &bool,
    no_start: &bool,
    native: &bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    if *native {
        match lsw_core::verifyops::native_backtrace(&p, program)? {
            None => {
                eprintln!(
                    "no [verify] host configured in lsw.toml; native debugging needs a Windows host"
                );
                return Ok(ExitCode::FAILURE);
            }
            Some(bt) => {
                if format == Format::Json {
                    println!("{}", serde_json::to_string_pretty(&bt).expect("serializes"));
                } else {
                    println!("Native debug on {}", bt.host);
                    if let Some(e) = &bt.exception {
                        println!("Exception: {e}");
                    }
                    if bt.frames.is_empty() {
                        println!("(no stack frames captured; the program may not have crashed)");
                    } else {
                        println!("Backtrace:");
                        for f in &bt.frames {
                            println!("  #{:<2} {}", f.index, f.call_site);
                        }
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
        }
    }
    let status = lsw_core::debugops::debug(
        &env,
        Some(&p),
        program,
        args,
        &lsw_core::debugops::DebugOptions {
            gdb: *gdb,
            no_start: *no_start,
        },
    )?;
    Ok(exit_from_status(status))
}

pub(crate) fn dap(dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    let (_p, env) = active_env(dirs)?;
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    lsw_core::dapops::serve(&env, &mut reader, &mut writer)?;
    Ok(ExitCode::SUCCESS)
}
