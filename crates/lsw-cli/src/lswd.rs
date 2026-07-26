use std::process::ExitCode;

use lsw_core::Dirs;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("lswd {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!(
                    "lswd {}\nBackground daemon for lsw; listens on a per-user unix socket.\n\nUsage: lswd\n\nStops via: lsw daemon stop\nStatus:    lsw daemon status",
                    env!("CARGO_PKG_VERSION")
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("lswd: unknown argument '{other}' (lswd takes no arguments)");
                return ExitCode::FAILURE;
            }
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    let dirs = match Dirs::resolve() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("lswd: {e}");
            return ExitCode::FAILURE;
        }
    };

    let socket = lsw_core::daemonops::socket_path(&dirs);
    eprintln!("lswd listening on {}", socket.display());
    match lsw_core::daemonops::serve(&dirs) {
        Ok(()) => {
            eprintln!("lswd stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lswd: {e}");
            ExitCode::FAILURE
        }
    }
}
