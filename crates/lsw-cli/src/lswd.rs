use std::process::ExitCode;

use lsw_core::Dirs;

fn main() -> ExitCode {
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
