use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

use lsw_config::ResolvedRuntime;

use crate::env::scrub_wine_env;
use crate::sandbox::{
    apply_rlimits, bwrap_args, find_bwrap, find_pasta, find_xvfb_run, sandbox_base_env,
    should_unshare_net,
};
use crate::types::{DisplayMode, ExecutionRequest, NetworkMode, RuntimeDiagnostics, RuntimeError};

/// Trait for Windows runtime providers (Wine, etc.).
pub trait RuntimeProvider {
    /// Returns the unique identifier (e.g. `"wine"`).
    fn id(&self) -> &'static str;

    /// Resolves the runtime executable and version.
    fn resolve(&self) -> Result<ResolvedRuntime, RuntimeError>;

    /// Initializes a Wine prefix at the given path.
    fn prepare(&self, prefix: &Path) -> Result<(), RuntimeError>;

    /// Executes a Windows program under this runtime.
    fn execute(&self, req: &ExecutionRequest) -> Result<ExitStatus, RuntimeError>;

    /// Kills a process running inside this runtime's prefix.
    fn kill(&self, prefix: &Path, pid: u32) -> Result<(), RuntimeError>;

    /// Returns diagnostic information about this runtime.
    fn diagnostics(&self, prefix: &Path) -> RuntimeDiagnostics;
}

const WINE_ID: &str = "wine";
pub(crate) const SYSTEM_REG: &str = "system.reg";
const WINESERVER_TIMEOUT: Duration = Duration::from_secs(30);

/// Wine runtime provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct WineRuntime;

/// Returns the default Wine environment variables for a given prefix.
pub fn base_env(prefix: &Path) -> Vec<(String, String)> {
    vec![
        ("WINEPREFIX".to_owned(), prefix.display().to_string()),
        ("WINEDEBUG".to_owned(), "fixme-all".to_owned()),
        (
            "WINEDLLOVERRIDES".to_owned(),
            "winemenubuilder.exe=d".to_owned(),
        ),
    ]
}

/// Returns true if the environment variable affects host process startup.
pub fn host_loader_sensitive(key: &str) -> bool {
    key.starts_with("LD_")
        || matches!(
            key,
            "BASH_ENV"
                | "ENV"
                | "GCONV_PATH"
                | "GETCONF_DIR"
                | "HOSTALIASES"
                | "NODE_OPTIONS"
                | "PATH"
                | "PERL5LIB"
                | "PERL5OPT"
                | "PYTHONHOME"
                | "PYTHONPATH"
                | "RUBYLIB"
                | "RUBYOPT"
                | "WINEDLLPATH"
                | "WINELOADER"
                | "WINESERVER"
                | "WINEPREFIX"
                | "WINEPATH"
                | "WINEARCH"
        )
}

pub(crate) fn full_env(prefix: &Path, extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = base_env(prefix);
    env.extend(crate::gpu::egl_vendor_pin());
    env.extend(
        extra
            .iter()
            .filter(|(key, _)| !host_loader_sensitive(key))
            .cloned(),
    );
    env
}

pub(crate) fn find_in_paths(name: &str, paths: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(paths).find_map(|dir| {
        if dir.as_os_str().is_empty() {
            return None;
        }
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            std::path::absolute(&candidate).ok()
        } else {
            None
        }
    })
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

pub(crate) fn find_wine() -> Option<PathBuf> {
    if std::env::var_os("LSW_WINE").is_some() {
        return wine_override().ok();
    }
    let path_var = std::env::var_os("PATH")?;
    find_in_paths(WINE_ID, &path_var)
}

fn wine_override() -> Result<PathBuf, RuntimeError> {
    let Some(raw) = std::env::var_os("LSW_WINE") else {
        return Err(RuntimeError::WineNotFound);
    };
    let path = PathBuf::from(raw);
    if is_executable_file(&path) {
        std::path::absolute(&path).map_err(|_| RuntimeError::WineOverrideInvalid { path })
    } else {
        Err(RuntimeError::WineOverrideInvalid { path })
    }
}

pub(crate) fn parse_wine_version(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed.strip_prefix("wine-").unwrap_or(trimmed).to_owned()
}

impl WineRuntime {
    fn wine_executable() -> Result<PathBuf, RuntimeError> {
        if std::env::var_os("LSW_WINE").is_some() {
            return wine_override();
        }
        find_wine().ok_or(RuntimeError::WineNotFound)
    }

    /// Gracefully shuts down the wineserver for a prefix.
    pub fn shutdown_prefix(&self, prefix: &Path) -> Result<(), RuntimeError> {
        let wineserver = Self::wineserver_executable()?;
        for flag in ["-k", "-w"] {
            let mut command = command_with_prefix(&wineserver, prefix);
            command.arg(flag);
            let status = wait_with_timeout(&mut command, &wineserver, WINESERVER_TIMEOUT)?;
            if !status.success() {
                return Err(RuntimeError::ExecutionFailed {
                    detail: format!("'{} {flag}' exited with {status}", wineserver.display()),
                });
            }
        }
        Ok(())
    }

    fn wineserver_executable() -> Result<PathBuf, RuntimeError> {
        let wine = Self::wine_executable()?;
        if let Some(sibling) = wine.parent().map(|d| d.join("wineserver"))
            && sibling.is_file()
        {
            return Ok(sibling);
        }
        if let Some(path_var) = std::env::var_os("PATH")
            && let Some(found) = find_in_paths("wineserver", &path_var)
        {
            return Ok(found);
        }
        Err(RuntimeError::PrefixInitFailed {
            detail:
                "wineserver not found next to wine or on PATH; cannot safely settle prefix state"
                    .into(),
        })
    }

    /// Spawns a Wine process without waiting for it.
    pub fn spawn(&self, req: &ExecutionRequest) -> Result<std::process::Child, RuntimeError> {
        let mut command = self.command(req)?;
        command.spawn().map_err(|source| RuntimeError::SpawnFailed {
            program: PathBuf::from(command.get_program()),
            source,
        })
    }

    /// Builds the Wine command without spawning it.
    pub fn command(&self, req: &ExecutionRequest) -> Result<Command, RuntimeError> {
        let (loader, executable) = match &req.emulate {
            Some(em) => (Some(em.qemu.clone()), em.wine.clone()),
            None => (None, Self::wine_executable()?),
        };

        let virtual_display = req.display == DisplayMode::Virtual;
        let sandboxed = req.sandbox.is_some();
        let mut argv: Vec<std::ffi::OsString> = Vec::new();

        if virtual_display {
            let xvfb = find_xvfb_run().ok_or(RuntimeError::VirtualDisplayUnavailable)?;
            argv.push(xvfb.into_os_string());
            argv.push("-a".into());
            argv.push("--".into());
        }

        if let Some(spec) = &req.sandbox {
            let bwrap = find_bwrap().ok_or(RuntimeError::SandboxUnavailable)?;
            let pasta = if spec.network == NetworkMode::Isolated {
                find_pasta()
            } else {
                None
            };
            let unshare_net = should_unshare_net(spec.network, pasta.is_some());
            if let Some(pasta) = &pasta {
                argv.push(pasta.clone().into_os_string());
                argv.push("--config-net".into());
                argv.push("--".into());
            }
            argv.push(bwrap.into_os_string());
            argv.extend(bwrap_args(spec, unshare_net).into_iter().map(Into::into));
            if virtual_display {
                for a in ["--ro-bind", "/tmp/.X11-unix", "/tmp/.X11-unix"] {
                    argv.push(a.into());
                }
            }
        }

        if let Some(loader) = &loader {
            argv.push(loader.clone().into_os_string());
        }
        argv.push(executable.into_os_string());
        argv.push(req.program.clone().into_os_string());
        argv.extend(req.args.iter().map(Into::into));

        let (head, tail) = argv.split_first().expect("argv always has wine at minimum");
        let mut command = Command::new(head);
        command.args(tail);

        if sandboxed {
            command.env_clear();
            for (key, value) in sandbox_base_env() {
                command.env(key, value);
            }
            command.envs(full_env(&req.prefix, &req.env));
            if let Some(spec) = &req.sandbox {
                apply_rlimits(&mut command, spec);
            }
        } else {
            scrub_wine_env(&mut command);
            command.envs(full_env(&req.prefix, &req.env));
        }
        if let Some(cwd) = &req.cwd {
            if !cwd.is_dir() {
                return Err(RuntimeError::ExecutionFailed {
                    detail: format!("working directory {} does not exist", cwd.display()),
                });
            }
            command.current_dir(cwd);
        }
        tracing::debug!(program = %req.program.display(), prefix = %req.prefix.display(), sandboxed, virtual_display, "executing via wine");
        Ok(command)
    }
}

fn command_with_prefix(program: &Path, prefix: &Path) -> Command {
    let mut command = Command::new(program);
    scrub_wine_env(&mut command);
    command.env("WINEPREFIX", prefix.as_os_str());
    command
}

fn wait_with_timeout(
    command: &mut Command,
    program: &Path,
    timeout: Duration,
) -> Result<ExitStatus, RuntimeError> {
    let mut child = command
        .spawn()
        .map_err(|source| RuntimeError::SpawnFailed {
            program: program.to_path_buf(),
            source,
        })?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RuntimeError::ExecutionFailed {
                    detail: format!(
                        "'{}' did not exit within {} seconds",
                        program.display(),
                        timeout.as_secs()
                    ),
                });
            }
            Err(source) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RuntimeError::ExecutionFailed {
                    detail: format!("waiting for '{}' failed: {source}", program.display()),
                });
            }
        }
    }
}

impl RuntimeProvider for WineRuntime {
    fn id(&self) -> &'static str {
        WINE_ID
    }

    fn resolve(&self) -> Result<ResolvedRuntime, RuntimeError> {
        let executable = Self::wine_executable()?;
        let output = Command::new(&executable)
            .arg("--version")
            .output()
            .map_err(|source| RuntimeError::SpawnFailed {
                program: executable.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(RuntimeError::ExecutionFailed {
                detail: format!(
                    "'{} --version' exited with {}",
                    executable.display(),
                    output.status
                ),
            });
        }
        let version = parse_wine_version(&String::from_utf8_lossy(&output.stdout));
        Ok(ResolvedRuntime {
            provider: WINE_ID.to_owned(),
            version,
            executable,
        })
    }

    fn prepare(&self, prefix: &Path) -> Result<(), RuntimeError> {
        let executable = Self::wine_executable()?;
        std::fs::create_dir_all(prefix).map_err(|e| RuntimeError::PrefixInitFailed {
            detail: format!("cannot create prefix directory {}: {e}", prefix.display()),
        })?;

        tracing::debug!(prefix = %prefix.display(), "initializing wine prefix via wineboot -u");
        let mut command = Command::new(&executable);
        scrub_wine_env(&mut command);
        let output = command
            .args(["wineboot", "-u"])
            .envs(base_env(prefix))
            .output()
            .map_err(|source| RuntimeError::SpawnFailed {
                program: executable.clone(),
                source,
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RuntimeError::PrefixInitFailed {
                detail: format!(
                    "'wineboot -u' exited with {} in prefix {}: {}",
                    output.status,
                    prefix.display(),
                    stderr.trim()
                ),
            });
        }
        Ok(())
    }

    fn execute(&self, req: &ExecutionRequest) -> Result<ExitStatus, RuntimeError> {
        let mut child = self.spawn(req)?;
        child
            .wait()
            .map_err(|source| RuntimeError::ExecutionFailed {
                detail: format!("waiting for {} failed: {source}", req.program.display()),
            })
    }

    fn kill(&self, prefix: &Path, pid: u32) -> Result<(), RuntimeError> {
        crate::env::kill_validated(pid, prefix)
    }

    fn diagnostics(&self, prefix: &Path) -> RuntimeDiagnostics {
        let resolved = self.resolve().ok();
        RuntimeDiagnostics {
            id: WINE_ID.to_owned(),
            version: resolved.as_ref().map(|r| r.version.clone()),
            executable: resolved.map(|r| r.executable),
            prefix_exists: prefix.is_dir(),
            prefix_initialized: prefix.join(SYSTEM_REG).is_file(),
        }
    }
}

#[cfg(test)]
mod wine_tests {
    use super::*;

    fn script(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("command");
        std::fs::write(&path, format!("{body}\n")).unwrap();
        (dir, path)
    }

    #[test]
    fn wait_with_timeout_reports_nonzero_status() {
        let (_dir, path) = script("exit 7");
        let status = wait_with_timeout(
            Command::new("/bin/sh").arg(&path).arg("-k"),
            Path::new("/bin/sh"),
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(status.code(), Some(7));
    }

    #[test]
    fn wait_with_timeout_kills_stuck_process() {
        let (_dir, path) = script("while :; do :; done");
        let error = wait_with_timeout(
            Command::new("/bin/sh").arg(&path).arg("-w"),
            Path::new("/bin/sh"),
            Duration::from_millis(25),
        )
        .unwrap_err();
        assert!(error.to_string().contains("did not exit"));
    }
}
