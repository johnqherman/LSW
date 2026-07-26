use std::process::ExitCode;

use lsw_core::Dirs;

use crate::active_env;
use crate::cli::{DaemonCmd, Format, IdeCmd, PluginCmd};

pub(crate) fn ide(op: &IdeCmd, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    match op {
        IdeCmd::Env => {
            let (p, env) = active_env(dirs)?;
            let description = lsw_core::ideops::ide_env(&env, Some(&p))?;
            crate::cmd::emit_json(&description);
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn plugin(op: &PluginCmd, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        PluginCmd::List => {
            let discovered = lsw_core::pluginops::discover();
            let json = format == Format::Json;
            let mut any_failed = false;
            let mut rows = Vec::new();
            for d in &discovered {
                match lsw_core::pluginops::Plugin::connect(&d.name, &d.path) {
                    Ok(plugin) => {
                        let h = &plugin.handshake;
                        if json {
                            rows.push(serde_json::json!({
                                "name": d.name,
                                "version": h.provider_version,
                                "kind": h.kind,
                                "protocol": h.protocol,
                                "path": d.path.display().to_string(),
                            }));
                        } else {
                            println!(
                                "{:<16} {:<10} {:<8} proto v{}  {}",
                                d.name,
                                h.provider_version,
                                h.kind,
                                h.protocol,
                                d.path.display()
                            );
                        }
                        plugin.shutdown();
                    }
                    Err(e) => {
                        any_failed = true;
                        if json {
                            rows.push(serde_json::json!({
                                "name": d.name,
                                "error": e.to_string(),
                            }));
                        } else {
                            println!("{:<16} ERROR: {e}", d.name);
                        }
                    }
                }
            }
            if json {
                crate::cmd::emit_json(&rows);
            } else if discovered.is_empty() {
                println!(
                    "No provider plugins found (looked for lsw-provider-* on PATH, protocol v{})",
                    lsw_core::pluginops::PROTOCOL_VERSION
                );
            }
            crate::cmd::exit_ok(!any_failed)
        }
    }
}

pub(crate) fn daemon(op: &DaemonCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let json = format == Format::Json;
    match op {
        DaemonCmd::Status => {
            let probe = lsw_core::daemonops::DaemonClient::connect(dirs)
                .and_then(|mut c| c.call("version"));
            match probe {
                Ok(v) => {
                    if json {
                        crate::cmd::emit_json(&serde_json::json!({
                            "running": true,
                            "protocol": v["protocol"],
                            "version": v["version"],
                        }));
                    } else {
                        println!(
                            "lswd running (protocol v{}, version {})",
                            v["protocol"], v["version"]
                        );
                    }
                }
                Err(_) => {
                    if json {
                        crate::cmd::emit_json(&serde_json::json!({ "running": false }));
                    } else {
                        println!("lswd not running (start it with: lswd)");
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        DaemonCmd::Stop => {
            let mut client = lsw_core::daemonops::DaemonClient::connect(dirs)?;
            client.call("shutdown")?;
            if json {
                crate::cmd::emit_json(&serde_json::json!({ "stopping": true }));
            } else {
                println!("lswd stopping");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
