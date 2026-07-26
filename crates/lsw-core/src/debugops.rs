use std::path::Path;
use std::process::{Command, ExitStatus};

use crate::envops::Environment;
use crate::error::{Error, Result};

#[derive(Debug, Default)]
pub struct DebugOptions {
    pub gdb: bool,
    pub no_start: bool,
}

pub fn attach(env: &Environment, pid: u32, gdb: bool) -> Result<ExitStatus> {
    let known = crate::psops::ps(env)?.iter().any(|p| p.pid == pid);
    if !known {
        return Err(Error::ProcessNotInEnvironment {
            pid,
            environment: env.name.clone(),
        });
    }
    let winedbg = crate::buildops::which("winedbg").ok_or_else(|| Error::ToolMissing {
        tool: "winedbg".into(),
        fix: "install wine (winedbg ships with it)".into(),
    })?;
    let mut command = Command::new(&winedbg);
    lsw_runtime::scrub_wine_env(&mut command);
    if gdb {
        command.arg("--gdb");
    }
    command.arg(pid.to_string());
    command.env("WINEPREFIX", env.layout.prefix());
    command.env("WINEDEBUG", "fixme-all");
    command.status().map_err(|e| Error::io(winedbg.clone(), e))
}

pub fn debug(
    env: &Environment,
    project: Option<&crate::project::Project>,
    program: &Path,
    args: &[String],
    opts: &DebugOptions,
) -> Result<ExitStatus> {
    if !program.is_file() {
        return Err(Error::NotExecutable {
            program: program.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let program = std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;

    if let Some(p) = project {
        crate::buildops::check_lock(p, env)?;
    }

    let winedbg = crate::buildops::which("winedbg").ok_or_else(|| Error::ToolMissing {
        tool: "winedbg".into(),
        fix: "install wine (winedbg ships with it)".into(),
    })?;

    let mut command = Command::new(&winedbg);
    lsw_runtime::scrub_wine_env(&mut command);
    if opts.gdb {
        command.arg("--gdb");
        if opts.no_start {
            command.arg("--no-start");
        }
    }
    command.arg(&program).args(args);
    command.env("WINEPREFIX", env.layout.prefix());
    command.env("WINEDEBUG", "fixme-all");
    command.env("WINEDLLOVERRIDES", "winemenubuilder.exe=d");

    command.status().map_err(|e| Error::io(winedbg.clone(), e))
}
