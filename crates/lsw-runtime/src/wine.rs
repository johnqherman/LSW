use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use lsw_config::ResolvedRuntime;

use crate::env::{process_in_prefix, scrub_host_wine_vars};
use crate::sandbox::{
    apply_rlimits, bwrap_args, find_bwrap, find_pasta, find_xvfb_run, sandbox_base_env,
    should_unshare_net,
};
use crate::types::{DisplayMode, ExecutionRequest, NetworkMode, RuntimeDiagnostics, RuntimeError};

pub trait RuntimeProvider {
    fn id(&self) -> &'static str;

    fn resolve(&self) -> Result<ResolvedRuntime, RuntimeError>;

    fn prepare(&self, prefix: &Path) -> Result<(), RuntimeError>;

    fn execute(&self, req: &ExecutionRequest) -> Result<ExitStatus, RuntimeError>;

    fn kill(&self, prefix: &Path, pid: u32) -> Result<(), RuntimeError>;

    fn diagnostics(&self, prefix: &Path) -> RuntimeDiagnostics;
}

pub fn providers() -> Vec<Box<dyn RuntimeProvider>> {
    vec![Box::new(WineRuntime)]
}

const WINE_ID: &str = "wine";
pub(crate) const SYSTEM_REG: &str = "system.reg";

#[derive(Debug, Clone, Copy, Default)]
pub struct WineRuntime;

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

pub(crate) fn full_env(prefix: &Path, extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = base_env(prefix);
    env.extend(extra.iter().cloned());
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
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub(crate) fn find_wine() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    find_in_paths(WINE_ID, &path_var)
}

pub(crate) fn parse_wine_version(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed.strip_prefix("wine-").unwrap_or(trimmed).to_owned()
}

impl WineRuntime {
    fn wine_executable() -> Result<PathBuf, RuntimeError> {
        find_wine().ok_or(RuntimeError::WineNotFound)
    }

    pub fn shutdown_prefix(&self, prefix: &Path) -> Result<(), RuntimeError> {
        let wineserver = Self::wineserver_executable()?;
        for flag in ["-k", "-w"] {
            let status = command_with_prefix(&wineserver, prefix)
                .arg(flag)
                .status()
                .map_err(|source| RuntimeError::SpawnFailed {
                    program: wineserver.clone(),
                    source,
                })?;
            let _ = status;
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
}

fn command_with_prefix(program: &Path, prefix: &Path) -> Command {
    let mut command = Command::new(program);
    scrub_host_wine_vars(&mut command);
    command.env("WINEPREFIX", prefix.as_os_str());
    command
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
        scrub_host_wine_vars(&mut command);
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
        argv.push(executable.clone().into_os_string());
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
            scrub_host_wine_vars(&mut command);
            command.envs(full_env(&req.prefix, &req.env));
        }
        if let Some(cwd) = &req.cwd {
            command.current_dir(cwd);
        }
        tracing::debug!(program = %req.program.display(), prefix = %req.prefix.display(), sandboxed, virtual_display, "executing via wine");
        command
            .status()
            .map_err(|source| RuntimeError::SpawnFailed {
                program: req.program.clone(),
                source,
            })
    }

    fn kill(&self, prefix: &Path, pid: u32) -> Result<(), RuntimeError> {
        if !process_in_prefix(pid, prefix) {
            return Err(RuntimeError::ProcessNotInEnvironment { pid });
        }
        let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        if rc != 0 {
            return Err(RuntimeError::ProcessNotInEnvironment { pid });
        }
        Ok(())
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
