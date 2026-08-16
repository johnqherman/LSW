use std::path::Path;
use std::process::{Command, ExitStatus};

use crate::envops::Environment;
use crate::error::{Error, Result};

#[derive(Debug, Default)]
/// Debug Options.
pub struct DebugOptions {
    /// Gdb.
    pub gdb: bool,
    /// No start.
    pub no_start: bool,
}

/// Attach.
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

/// Debug.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_options_defaults() {
        let opts = DebugOptions::default();
        assert!(!opts.gdb);
        assert!(!opts.no_start);
    }

    #[test]
    fn debug_rejects_nonexistent_program() {
        use std::path::PathBuf;
        use lsw_config::*;
        let env = crate::envops::Environment {
            name: "test".into(),
            manifest: EnvironmentManifest {
                name: "test".into(),
                format: 1,
                target_arch: TargetArch::X86_64,
                toolchain: ResolvedToolchain {
                    provider: "gcc".into(),
                    version: "14".into(),
                    cc: PathBuf::from("/usr/bin/cc"),
                    cxx: PathBuf::from("/usr/bin/c++"),
                    sysroot: PathBuf::from("/usr"),
                    c_flags: vec![],
                    cxx_flags: vec![],
                    link_flags: vec![],
                },
                runtime: ResolvedRuntime {
                    provider: "wine".into(),
                    version: "9.0".into(),
                    executable: PathBuf::from("/usr/bin/wine"),
                },
            },
            layout: EnvironmentLayout::new(PathBuf::from("/tmp/lsw-test-debug")),
        };
        let result = debug(
            &env,
            None,
            Path::new("/nonexistent/program.exe"),
            &[],
            &DebugOptions::default(),
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotExecutable { .. }));
    }
}
