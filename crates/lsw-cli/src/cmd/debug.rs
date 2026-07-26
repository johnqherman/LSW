use std::path::Path;
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::Format;
use crate::{active_env, exit_from_status};

pub(crate) struct DebugFlags {
    pub(crate) gdb: bool,
    pub(crate) no_start: bool,
    pub(crate) native: bool,
    pub(crate) analyze: bool,
    pub(crate) interactive: bool,
}

pub(crate) fn debug(
    program: &Path,
    args: &[String],
    flags: &DebugFlags,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    if flags.native {
        let cfg = &p.manifest.verify;
        if cfg.host.is_some() {
            match cfg.transport.as_deref().unwrap_or("ssh") {
                "ssh" => {}
                "winrm" | "https" => {
                    return Ok(crate::usage_failure(
                        format,
                        "native debugging (backtrace) is only supported over ssh; winrm/https hosts can run binaries with `lsw compat --native` but cannot capture backtraces",
                    ));
                }
                other => {
                    return Err(lsw_core::Error::UnsupportedTransport {
                        transport: other.to_owned(),
                    });
                }
            }
        }
        const NO_HOST: &str =
            "no [verify] host configured in lsw.toml; native debugging needs a Windows host";
        if flags.interactive {
            return match lsw_core::verifyops::native_interactive(&p, program)? {
                None => Ok(crate::usage_failure(format, NO_HOST)),
                Some(status) => Ok(crate::exit_from_status(status)),
            };
        }
        if flags.analyze {
            return match lsw_core::verifyops::native_analyze(&p, program)? {
                None => Ok(crate::usage_failure(format, NO_HOST)),
                Some(a) => {
                    if format == Format::Json {
                        crate::cmd::emit_json(&a);
                    } else {
                        println!("Native crash analysis on {}", a.host);
                        if let Some(b) = &a.bucket_id {
                            println!("Bucket:    {b}");
                        }
                        if let Some(c) = &a.failure_class {
                            println!("Exception: {c}");
                        }
                        if let Some(s) = &a.symbol {
                            println!("Symbol:    {s}");
                        }
                        if let Some(i) = &a.image {
                            println!("Image:     {i}");
                        }
                        if a.bucket_id.is_none() && a.frames.is_empty() {
                            println!("(no failure bucket; the program may not have crashed)");
                        }
                        if !a.frames.is_empty() {
                            println!("Stack:");
                            for f in &a.frames {
                                println!("  #{:<2} {}", f.index, f.call_site);
                            }
                        }
                    }
                    Ok(ExitCode::SUCCESS)
                }
            };
        }
        match lsw_core::verifyops::native_backtrace(&p, program)? {
            None => {
                return Ok(crate::usage_failure(format, NO_HOST));
            }
            Some(bt) => {
                if format == Format::Json {
                    crate::cmd::emit_json(&bt);
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
            gdb: flags.gdb,
            no_start: flags.no_start,
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
