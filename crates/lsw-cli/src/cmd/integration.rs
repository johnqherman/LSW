use std::process::ExitCode;

use lsw_core::Dirs;

use crate::active_env;
use crate::cli::{DaemonCmd, IdeCmd, PluginCmd};

pub(crate) fn ide(op: &IdeCmd, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    match op {
        IdeCmd::Env => {
            let (p, env) = active_env(dirs)?;
            let description = lsw_core::ideops::ide_env(&env, Some(&p))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&description).expect("serializes")
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn plugin(op: &PluginCmd) -> lsw_core::Result<ExitCode> {
    match op {
        PluginCmd::List => {
            let discovered = lsw_core::pluginops::discover();
            if discovered.is_empty() {
                println!(
                    "No provider plugins found (looked for lsw-provider-* on PATH, protocol v{})",
                    lsw_core::pluginops::PROTOCOL_VERSION
                );
            }
            for d in discovered {
                match lsw_core::pluginops::Plugin::connect(&d.name, &d.path) {
                    Ok(plugin) => {
                        let h = &plugin.handshake;
                        println!(
                            "{:<16} {:<10} {:<8} proto v{}  {}",
                            d.name,
                            h.provider_version,
                            h.kind,
                            h.protocol,
                            d.path.display()
                        );
                        plugin.shutdown();
                    }
                    Err(e) => println!("{:<16} ERROR: {e}", d.name),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn daemon(op: &DaemonCmd, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    match op {
        DaemonCmd::Status => {
            let probe = lsw_core::daemonops::DaemonClient::connect(dirs)
                .and_then(|mut c| c.call("version"));
            match probe {
                Ok(v) => println!(
                    "lswd running (protocol v{}, version {})",
                    v["protocol"], v["version"]
                ),
                Err(_) => println!("lswd not running (start it with: lswd)"),
            }
            Ok(ExitCode::SUCCESS)
        }

        DaemonCmd::Stop => {
            let mut client = lsw_core::daemonops::DaemonClient::connect(dirs)?;
            client.call("shutdown")?;
            println!("lswd stopping");
            Ok(ExitCode::SUCCESS)
        }
    }
}
