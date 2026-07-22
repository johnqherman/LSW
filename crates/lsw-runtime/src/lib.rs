use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use lsw_config::ResolvedRuntime;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(
        "LSW1501: wine executable not found on PATH; \
         install wine via your package manager (e.g. 'pacman -S wine' or 'apt install wine')"
    )]
    WineNotFound,

    #[error(
        "LSW1502: wine prefix initialization failed: {detail}; \
         delete the prefix directory and re-run, or run 'wineboot -u' manually with WINEPREFIX set to inspect the failure"
    )]
    PrefixInitFailed { detail: String },

    #[error(
        "LSW1503: cannot spawn {}: {source}; \
         check that the file exists and the runtime is installed correctly", program.display()
    )]
    SpawnFailed {
        program: PathBuf,
        source: std::io::Error,
    },

    #[error(
        "LSW1505: strict sandbox requested but bubblewrap (bwrap) is not installed; \
         install bubblewrap or drop --sandbox"
    )]
    SandboxUnavailable,

    #[error(
        "LSW1506: a virtual display was requested but xvfb-run is not installed; \
         install xvfb (the 'xorg-server-xvfb' or 'xvfb' package) or run with a real $DISPLAY"
    )]
    VirtualDisplayUnavailable,

    #[error(
        "LSW1504: runtime execution failed: {detail}; \
         re-run with WINEDEBUG unset (pass it in the request env) for more diagnostics"
    )]
    ExecutionFailed { detail: String },

    #[error("LSW1507: process {pid} is not running in this environment")]
    ProcessNotInEnvironment { pid: u32 },
}

pub fn process_in_prefix(pid: u32, prefix: &Path) -> bool {
    let Ok(environ) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    let needle = format!("WINEPREFIX={}", prefix.display());
    environ
        .split(|b| *b == 0)
        .any(|entry| entry == needle.as_bytes())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub prefix: PathBuf,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub sandbox: Option<SandboxSpec>,
    pub display: DisplayMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    #[default]
    Inherit,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkMode {
    Host,
    Isolated,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SandboxSpec {
    pub rw_binds: Vec<PathBuf>,
    pub network: NetworkMode,
    pub cpu_seconds: Option<u64>,
    pub memory_bytes: Option<u64>,
}

pub fn find_pasta() -> Option<PathBuf> {
    find_on_path("pasta").or_else(|| find_on_path("slirp4netns"))
}

fn should_unshare_net(mode: NetworkMode, pasta_available: bool) -> bool {
    match mode {
        NetworkMode::Host => false,
        NetworkMode::None => true,
        NetworkMode::Isolated => !pasta_available,
    }
}

pub fn bwrap_args(spec: &SandboxSpec, unshare_net: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--die-with-parent",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/etc",
        "/etc",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib64",
        "/lib64",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/bin",
        "/sbin",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-ipc",
        "--new-session",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    if let Some(home) = dirs_home() {
        args.push("--tmpfs".into());
        args.push(home.display().to_string());
    }
    for path in &spec.rw_binds {
        let p = path.display().to_string();
        args.push("--bind".into());
        args.push(p.clone());
        args.push(p);
    }
    if unshare_net {
        args.push("--unshare-net".into());
    }
    args
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn apply_rlimits(command: &mut Command, spec: &SandboxSpec) {
    let cpu = spec.cpu_seconds;
    let mem = spec.memory_bytes;
    if cpu.is_none() && mem.is_none() {
        return;
    }
    unsafe {
        command.pre_exec(move || {
            if let Some(secs) = cpu {
                let lim = libc::rlimit {
                    rlim_cur: secs,
                    rlim_max: secs,
                };
                libc::setrlimit(libc::RLIMIT_CPU, &lim);
            }
            if let Some(bytes) = mem {
                let lim = libc::rlimit {
                    rlim_cur: bytes,
                    rlim_max: bytes,
                };
                libc::setrlimit(libc::RLIMIT_AS, &lim);
            }
            Ok(())
        });
    }
}

fn sandbox_base_env() -> Vec<(String, String)> {
    const ALLOW: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "TERM",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "DISPLAY",
        "XAUTHORITY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
    ];
    let mut out = Vec::new();
    for key in ALLOW {
        if let Some(value) = std::env::var_os(key)
            && let Ok(value) = value.into_string()
        {
            out.push(((*key).to_owned(), value));
        }
    }
    if !out.iter().any(|(k, _)| k == "PATH") {
        out.push(("PATH".to_owned(), "/usr/bin:/bin".to_owned()));
    }
    out
}

pub fn find_bwrap() -> Option<PathBuf> {
    find_on_path("bwrap")
}

pub fn find_xvfb_run() -> Option<PathBuf> {
    find_on_path("xvfb-run")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|c| c.is_file())
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDiagnostics {
    pub id: String,
    pub version: Option<String>,
    pub executable: Option<PathBuf>,
    pub prefix_exists: bool,
    pub prefix_initialized: bool,
}

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
const SYSTEM_REG: &str = "system.reg";

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

const HOST_WINE_VARS: &[&str] = &[
    "WINEPREFIX",
    "WINEARCH",
    "WINEPATH",
    "WINEDLLPATH",
    "WINEDLLOVERRIDES",
    "WINESERVER",
    "WINELOADER",
    "WINEDEBUG",
    "WINEFSYNC",
    "WINEESYNC",
];

fn scrub_host_wine_vars(command: &mut Command) {
    for var in HOST_WINE_VARS {
        command.env_remove(var);
    }
}

pub fn scrub_wine_env(command: &mut Command) {
    scrub_host_wine_vars(command);
}

fn full_env(prefix: &Path, extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = base_env(prefix);
    env.extend(extra.iter().cloned());
    env
}

fn find_in_paths(name: &str, paths: &OsStr) -> Option<PathBuf> {
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

fn find_wine() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    find_in_paths(WINE_ID, &path_var)
}

fn parse_wine_version(raw: &str) -> String {
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
        let executable = Self::wine_executable()?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bwrap_args_lock_down_the_filesystem_and_namespaces() {
        let spec = SandboxSpec {
            rw_binds: vec![PathBuf::from("/data/env"), PathBuf::from("/home/u/proj")],
            network: NetworkMode::None,
            ..Default::default()
        };
        let args = bwrap_args(&spec, true);
        let ro_usr = args.windows(3).any(|w| w == ["--ro-bind", "/usr", "/usr"]);
        assert!(ro_usr, "must ro-bind /usr");
        for flag in [
            "--unshare-pid",
            "--unshare-uts",
            "--unshare-ipc",
            "--unshare-net",
        ] {
            assert!(args.iter().any(|a| a == flag), "missing {flag}");
        }
        let bind_env = args
            .windows(3)
            .position(|w| w == ["--bind", "/data/env", "/data/env"]);
        assert!(bind_env.is_some(), "env dir must be writable");
        assert!(
            args.windows(3)
                .any(|w| w == ["--bind", "/home/u/proj", "/home/u/proj"]),
            "project dir must be writable"
        );
    }

    #[test]
    fn bwrap_args_keep_network_when_requested() {
        let spec = SandboxSpec {
            rw_binds: vec![],
            network: NetworkMode::Host,
            ..Default::default()
        };
        assert!(
            !bwrap_args(&spec, false)
                .iter()
                .any(|a| a == "--unshare-net")
        );
    }

    #[test]
    fn network_mode_decides_unshare_net() {
        assert!(!should_unshare_net(NetworkMode::Host, false));
        assert!(should_unshare_net(NetworkMode::None, true));
        assert!(!should_unshare_net(NetworkMode::Isolated, true));
        assert!(should_unshare_net(NetworkMode::Isolated, false));
    }

    fn skip_without_wine(test: &str) -> bool {
        if find_wine().is_none() {
            eprintln!("skipping {test}: wine not found on PATH");
            return true;
        }
        false
    }

    fn skip_without_wine_gate(test: &str) -> bool {
        if std::env::var("LSW_TEST_WINE").as_deref() != Ok("1") {
            eprintln!("skipping {test}: set LSW_TEST_WINE=1 to run wine integration tests");
            return true;
        }
        skip_without_wine(test)
    }

    #[test]
    fn resolve_finds_wine_with_version_and_absolute_path() {
        if skip_without_wine("resolve_finds_wine_with_version_and_absolute_path") {
            return;
        }
        let resolved = WineRuntime.resolve().unwrap();
        assert_eq!(resolved.provider, "wine");
        assert!(!resolved.version.is_empty(), "version must be non-empty");
        assert!(
            !resolved.version.starts_with("wine-"),
            "'wine-' prefix must be stripped, got {}",
            resolved.version
        );
        assert!(resolved.executable.is_absolute());
        assert!(resolved.executable.is_file());
    }

    #[test]
    fn find_in_paths_misses_on_empty_path() {
        assert_eq!(find_in_paths("wine", OsStr::new("")), None);
    }

    #[test]
    fn find_in_paths_ignores_non_executables_and_finds_executables() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("wine");
        std::fs::write(&plain, "not executable").unwrap();
        let paths = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(
            find_in_paths("wine", &paths),
            None,
            "mode 644 must not match"
        );

        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = find_in_paths("wine", &paths).unwrap();
        assert!(found.is_absolute());
        assert_eq!(found.file_name().unwrap(), "wine");
    }

    #[test]
    fn wine_not_found_error_is_lsw1501_and_actionable() {
        let msg = RuntimeError::WineNotFound.to_string();
        assert!(msg.starts_with("LSW1501"), "got: {msg}");
        assert!(msg.contains("package manager"), "must suggest a fix: {msg}");
    }

    #[test]
    fn error_ids_are_stable() {
        let spawn = RuntimeError::SpawnFailed {
            program: PathBuf::from("/x/app.exe"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        };
        assert!(spawn.to_string().starts_with("LSW1503"));
        let init = RuntimeError::PrefixInitFailed {
            detail: "boom".into(),
        };
        assert!(init.to_string().starts_with("LSW1502"));
        assert!(init.to_string().contains("boom"));
        let exec = RuntimeError::ExecutionFailed {
            detail: "boom".into(),
        };
        assert!(exec.to_string().starts_with("LSW1504"));
    }

    #[test]
    fn parse_wine_version_strips_prefix() {
        assert_eq!(parse_wine_version("wine-11.12\n"), "11.12");
        assert_eq!(parse_wine_version("wine-10.0 (Staging)"), "10.0 (Staging)");
        assert_eq!(parse_wine_version("11.12"), "11.12");
        assert_eq!(parse_wine_version(""), "");
    }

    #[test]
    fn base_env_sets_prefix_debug_and_dll_overrides() {
        let env = base_env(Path::new("/data/lsw/environments/e1/prefix"));
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .unwrap_or_else(|| panic!("{key} missing"))
        };
        assert_eq!(lookup("WINEPREFIX"), "/data/lsw/environments/e1/prefix");
        assert_eq!(lookup("WINEDEBUG"), "fixme-all");
        assert_eq!(lookup("WINEDLLOVERRIDES"), "winemenubuilder.exe=d");
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn full_env_keeps_caller_entries_last_so_they_override() {
        let extra = vec![
            ("WINEDEBUG".to_owned(), "+loaddll".to_owned()),
            ("MY_VAR".to_owned(), "1".to_owned()),
        ];
        let env = full_env(Path::new("/p"), &extra);
        let base_len = base_env(Path::new("/p")).len();
        assert_eq!(&env[base_len..], &extra[..]);
        let last_winedebug = env
            .iter()
            .rev()
            .find(|(k, _)| k == "WINEDEBUG")
            .map(|(_, v)| v.as_str());
        assert_eq!(last_winedebug, Some("+loaddll"));
    }

    #[test]
    fn kill_rejects_process_outside_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let err = WineRuntime
            .kill(dir.path(), std::process::id())
            .unwrap_err();
        assert!(err.to_string().starts_with("LSW1507"));
    }

    #[test]
    fn diagnostics_on_nonexistent_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-prefix");
        let diag = WineRuntime.diagnostics(&missing);
        assert_eq!(diag.id, "wine");
        assert!(!diag.prefix_exists);
        assert!(!diag.prefix_initialized);
    }

    #[test]
    fn diagnostics_on_initialized_looking_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SYSTEM_REG), "WINE REGISTRY Version 2\n").unwrap();
        let diag = WineRuntime.diagnostics(dir.path());
        assert!(diag.prefix_exists);
        assert!(diag.prefix_initialized);
    }

    #[test]
    fn diagnostics_reports_version_and_executable_when_wine_present() {
        if skip_without_wine("diagnostics_reports_version_and_executable_when_wine_present") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let diag = WineRuntime.diagnostics(dir.path());
        assert!(diag.version.is_some_and(|v| !v.is_empty()));
        assert!(diag.executable.is_some_and(|e| e.is_absolute()));
    }

    #[test]
    fn providers_lists_wine_first() {
        let all = providers();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id(), "wine");
    }

    #[test]
    fn gated_prepare_is_idempotent_and_execute_runs_cmd() {
        if skip_without_wine_gate("gated_prepare_is_idempotent_and_execute_runs_cmd") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("prefix");

        WineRuntime.prepare(&prefix).unwrap();
        WineRuntime.prepare(&prefix).unwrap();

        let diag = WineRuntime.diagnostics(&prefix);
        assert!(diag.prefix_exists);
        assert!(diag.prefix_initialized, "system.reg missing after wineboot");

        let status = WineRuntime
            .execute(&ExecutionRequest {
                program: PathBuf::from("cmd.exe"),
                args: vec!["/c".into(), "exit".into(), "0".into()],
                prefix: prefix.clone(),
                cwd: Some(dir.path().to_path_buf()),
                env: Vec::new(),
                sandbox: None,
                display: DisplayMode::Inherit,
            })
            .unwrap();
        assert!(status.success(), "cmd.exe /c exit 0 failed: {status}");
    }
}
