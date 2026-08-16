use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use lsw_config::TargetArch;
use lsw_pe::BinaryKind;
use lsw_runtime::{ExecutionRequest, RuntimeProvider, WineRuntime};

use crate::buildops;
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Host,
    Windows,
    Auto,
}

#[derive(Debug)]
pub struct RunReport {
    pub domain: Domain,
    pub status: ExitStatus,
}

#[derive(Debug)]
enum ResolvedProgram {
    HostPath(PathBuf),
    RuntimeResolved(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sandbox {
    #[default]
    None,
    Strict,
}

fn parse_network(value: &str) -> Option<lsw_runtime::NetworkMode> {
    match value {
        "host" => Some(lsw_runtime::NetworkMode::Host),
        "isolated" => Some(lsw_runtime::NetworkMode::Isolated),
        "none" => Some(lsw_runtime::NetworkMode::None),
        _ => None,
    }
}

fn is_unsafe_bind(canon: &std::path::Path) -> bool {
    const SYSTEM_ROOTS: &[&str] = &[
        "/", "/usr", "/etc", "/bin", "/sbin", "/lib", "/lib64", "/proc", "/dev", "/sys", "/boot",
        "/root", "/var", "/home", "/opt", "/srv",
    ];
    SYSTEM_ROOTS
        .iter()
        .any(|r| canon == std::path::Path::new(r))
}

fn sandbox_spec(
    env: &Environment,
    project: Option<&Project>,
    sandbox: Sandbox,
) -> Result<Option<lsw_runtime::SandboxSpec>> {
    match sandbox {
        Sandbox::None => Ok(None),
        Sandbox::Strict => {
            let mut raw_binds = vec![env.layout.root.clone()];
            if let Some(p) = project {
                raw_binds.push(p.root.clone());
            }
            let mut rw_binds = Vec::with_capacity(raw_binds.len());
            for bind in raw_binds {
                let canon = bind
                    .canonicalize()
                    .map_err(|_| Error::UnsafeSandboxBind { path: bind.clone() })?;
                if is_unsafe_bind(&canon) || canon.to_str().is_none() {
                    return Err(Error::UnsafeSandboxBind { path: canon });
                }
                rw_binds.push(canon);
            }
            let network = match project {
                Some(p) => parse_network(&p.manifest.sandbox.network).ok_or_else(|| {
                    Error::InvalidSandboxNetwork {
                        value: p.manifest.sandbox.network.clone(),
                    }
                })?,
                None => lsw_runtime::NetworkMode::Host,
            };
            let (cpu_seconds, memory_bytes) = project.map_or((None, None), |p| {
                (
                    p.manifest.sandbox.cpu_seconds,
                    p.manifest
                        .sandbox
                        .memory_mb
                        .map(|mb| mb.saturating_mul(1024 * 1024)),
                )
            });
            Ok(Some(lsw_runtime::SandboxSpec {
                rw_binds,
                network,
                cpu_seconds,
                memory_bytes,
            }))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    #[default]
    Auto,
    Inherit,
    Headless,
}

fn display_mode(control: Display, is_gui: Option<bool>) -> lsw_runtime::DisplayMode {
    use lsw_runtime::DisplayMode;
    match control {
        Display::Inherit => DisplayMode::Inherit,
        Display::Headless => DisplayMode::Virtual,
        Display::Auto => {
            let has_display = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
            if !has_display && is_gui == Some(true) {
                DisplayMode::Virtual
            } else {
                DisplayMode::Inherit
            }
        }
    }
}

pub fn run(
    env: &Environment,
    project: Option<&Project>,
    program: &Path,
    args: &[String],
    domain: Domain,
    sandbox: Sandbox,
    display: Display,
) -> Result<RunReport> {
    let resolved = resolve_program(program, domain)?;

    let mut is_gui: Option<bool> = None;
    let mut run_args = args.to_vec();
    let mut display_override = None;

    let (chosen, launch) = match resolved {
        ResolvedProgram::HostPath(p) if domain != Domain::Host && is_msi(&p) => {
            run_args = vec!["/i".to_owned(), z_drive_path(&p)];
            run_args.extend(args.iter().cloned());
            display_override = Some(lsw_runtime::DisplayMode::Inherit);
            (Domain::Windows, PathBuf::from("msiexec"))
        }
        ResolvedProgram::RuntimeResolved(p) => match domain {
            Domain::Host => {
                return Err(Error::NotExecutable {
                    program: p,
                    detail: "Windows-style paths cannot run in the host domain".into(),
                });
            }
            _ => (Domain::Windows, p),
        },
        ResolvedProgram::HostPath(p) => {
            let chosen = match domain {
                Domain::Auto => match lsw_pe::detect(&p)? {
                    BinaryKind::Pe(info) => {
                        is_gui = Some(info.subsystem == lsw_pe::Subsystem::Gui);
                        Domain::Windows
                    }
                    BinaryKind::Elf | BinaryKind::Script => Domain::Host,
                    BinaryKind::Unknown => {
                        return Err(Error::NotExecutable {
                            program: p,
                            detail: "not a PE, ELF, or script; pass --host or --windows to force"
                                .into(),
                        });
                    }
                },
                Domain::Windows => {
                    if let Ok(BinaryKind::Pe(info)) = lsw_pe::detect(&p) {
                        is_gui = Some(info.subsystem == lsw_pe::Subsystem::Gui);
                    }
                    Domain::Windows
                }
                Domain::Host => Domain::Host,
            };
            (chosen, p)
        }
    };

    let status = match chosen {
        Domain::Windows => {
            if let Some(p) = project {
                buildops::check_lock(p, env)?;
                crate::envops::link_project(env, p)?;
            }
            let resolved_display =
                display_override.unwrap_or_else(|| display_mode(display, is_gui));
            let mut child_env = windows_env(env, project);
            if resolved_display == lsw_runtime::DisplayMode::Virtual {
                child_env.push(("LSW_HEADLESS".to_owned(), "1".to_owned()));
            }
            WineRuntime.execute(&ExecutionRequest {
                program: launch,
                args: run_args,
                prefix: env.layout.prefix(),
                cwd: windows_cwd(env, project),
                env: child_env,
                sandbox: sandbox_spec(env, project, sandbox)?,
                display: resolved_display,
                emulate: crate::emulateops::resolve(env.manifest.target_arch)?,
            })?
        }
        Domain::Host | Domain::Auto => {
            if sandbox != Sandbox::None {
                return Err(Error::NotExecutable {
                    program: launch,
                    detail: "--sandbox applies to the Windows domain only".into(),
                });
            }
            Command::new(&launch)
                .args(args)
                .status()
                .map_err(|e| Error::io(launch.clone(), e))?
        }
    };

    Ok(RunReport {
        domain: chosen,
        status,
    })
}

pub fn windows_user() -> String {
    let raw = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default();
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "lsw".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn is_msi(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"))
}

pub(crate) fn z_drive_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

fn processor_architecture(arch: TargetArch) -> &'static str {
    match arch {
        TargetArch::X86_64 => "AMD64",
        TargetArch::X86 => "x86",
        TargetArch::Aarch64 | TargetArch::Arm64Ec => "ARM64",
        TargetArch::Armv7 => "ARM",
    }
}

fn env_overrides(
    section: &lsw_config::EnvSection,
    lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, value) in &section.vars {
        out.push((key.clone(), value.clone()));
    }
    for (key, host_var) in &section.secret {
        if let Some(value) = lookup(host_var) {
            out.push((key.clone(), value));
        }
    }
    out
}

pub(crate) fn spawn_in_prefix(
    env: &Environment,
    project: &Project,
    program: &Path,
) -> Result<std::process::Child> {
    WineRuntime
        .spawn(&lsw_runtime::ExecutionRequest {
            program: program.to_path_buf(),
            args: Vec::new(),
            prefix: env.layout.prefix(),
            cwd: windows_cwd(env, Some(project)),
            env: windows_env(env, Some(project)),
            sandbox: None,
            display: lsw_runtime::DisplayMode::Inherit,
            emulate: crate::emulateops::resolve(env.manifest.target_arch)?,
        })
        .map_err(Into::into)
}

fn windows_env(env: &Environment, project: Option<&Project>) -> Vec<(String, String)> {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let user = windows_user();
    let profile = format!("C:\\users\\{user}");
    let mut vars: Vec<(String, String)> = vec![
        ("TEMP".into(), "C:\\Temp".into()),
        ("TMP".into(), "C:\\Temp".into()),
        ("SystemRoot".into(), "C:\\windows".into()),
        ("windir".into(), "C:\\windows".into()),
        ("ComSpec".into(), "C:\\windows\\system32\\cmd.exe".into()),
        ("SystemDrive".into(), "C:".into()),
        ("ProgramFiles".into(), "C:\\Program Files".into()),
        ("ProgramFiles(x86)".into(), "C:\\Program Files (x86)".into()),
        ("ProgramData".into(), "C:\\ProgramData".into()),
        ("USERNAME".into(), user.clone()),
        ("USERPROFILE".into(), profile.clone()),
        ("HOMEDRIVE".into(), "C:".into()),
        ("HOMEPATH".into(), format!("\\users\\{user}")),
        ("APPDATA".into(), format!("{profile}\\AppData\\Roaming")),
        ("LOCALAPPDATA".into(), format!("{profile}\\AppData\\Local")),
        (
            "PROCESSOR_ARCHITECTURE".into(),
            processor_architecture(env.manifest.target_arch).into(),
        ),
        ("NUMBER_OF_PROCESSORS".into(), cpus.to_string()),
    ];
    if let Some(project) = project {
        vars.extend(env_overrides(&project.manifest.env, |k| {
            std::env::var(k).ok()
        }));
    }
    vars
}

fn windows_cwd(env: &Environment, project: Option<&Project>) -> Option<PathBuf> {
    let project = project?;
    let project_link = env.layout.src().join(&project.manifest.project.name);
    let cwd = std::env::current_dir().ok()?;
    let mapper = crate::envops::mapper(env, project);
    if let Ok(windows) = mapper.to_windows(&cwd)
        && let Some(rest) = windows.strip_prefix("C:\\")
    {
        if rest.is_empty() {
            return Some(env.layout.drive_c());
        }
        let candidate = env.layout.drive_c().join(rest.replace('\\', "/"));
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    Some(project_link).filter(|p| p.is_dir())
}

fn resolve_program(program: &Path, domain: Domain) -> Result<ResolvedProgram> {
    let text = program.to_string_lossy();
    if text.len() >= 2 && text.as_bytes()[1] == b':' {
        return Ok(ResolvedProgram::RuntimeResolved(program.to_path_buf()));
    }

    let has_separator = text.contains('/');
    if has_separator || program.is_file() {
        if !program.exists() {
            return Err(Error::NotExecutable {
                program: program.to_path_buf(),
                detail: "file not found".into(),
            });
        }
        if program.is_dir() {
            return Err(Error::NotExecutable {
                program: program.to_path_buf(),
                detail: "is a directory, not an executable".into(),
            });
        }
        let absolute =
            std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;
        return Ok(ResolvedProgram::HostPath(absolute));
    }

    if let Some(found) = buildops::which(&text) {
        let absolute = std::path::absolute(&found).map_err(|e| Error::io(found.clone(), e))?;
        return Ok(ResolvedProgram::HostPath(absolute));
    }
    if program.is_dir() {
        return Err(Error::NotExecutable {
            program: program.to_path_buf(),
            detail: "is a directory, not an executable".into(),
        });
    }
    if domain == Domain::Windows {
        return Ok(ResolvedProgram::RuntimeResolved(program.to_path_buf()));
    }
    Err(Error::NotExecutable {
        program: program.to_path_buf(),
        detail: "not found on PATH; pass --windows to let the runtime resolve Windows built-ins"
            .into(),
    })
}

fn safe_dos_path(path: &str) -> bool {
    path.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '\\' | ':' | '.' | '_' | '-' | '+' | '~' | ' ')
    })
}

fn dos_cwd(env: &Environment, project: Option<&Project>) -> Option<String> {
    let project = project?;
    let fallback = format!("C:\\src\\{}", project.manifest.project.name);
    let cwd = std::env::current_dir().ok()?;
    let windows = crate::envops::mapper(env, project)
        .to_windows(&cwd)
        .unwrap_or(fallback.clone());
    if windows.starts_with("C:\\") && safe_dos_path(&windows) {
        Some(windows)
    } else {
        Some(fallback)
    }
}

fn shell_invocation(powershell: bool, dos_cwd: Option<&str>) -> (PathBuf, Vec<String>) {
    if powershell {
        let mut args = vec!["-NoExit".to_owned(), "-Command".to_owned()];
        match dos_cwd {
            Some(dir) => args.extend([
                "Set-Location".to_owned(),
                "-LiteralPath".to_owned(),
                dir.to_owned(),
            ]),
            None => args.extend(["Set-Location".to_owned(), "$env:USERPROFILE".to_owned()]),
        }
        (PathBuf::from("powershell.exe"), args)
    } else {
        let mut args = vec!["/k".to_owned(), "cd".to_owned(), "/d".to_owned()];
        match dos_cwd {
            Some(dir) => args.push(dir.to_owned()),
            None => args.push("%USERPROFILE%".to_owned()),
        }
        (PathBuf::from("cmd.exe"), args)
    }
}

const WINE_BUILTIN_MARKER: &[u8; 16] = b"Wine builtin DLL";

pub(crate) fn is_real_windows_binary(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 80];
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    &head[64..80] != WINE_BUILTIN_MARKER
}

fn has_powershell(env: &Environment) -> bool {
    let path = env
        .layout
        .drive_c()
        .join("windows/system32/WindowsPowerShell/v1.0/powershell.exe");
    path.is_file() && is_real_windows_binary(&path)
}

static SHELL_CHILD_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static LAST_SIGINT_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
const SIGINT_EXIT_WINDOW_MS: i64 = 2000;

static SHELL_SESSION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

extern "C" fn shell_sigint(_: libc::c_int) {
    use std::sync::atomic::Ordering;
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut ts);
    }
    let now_ms = ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000;
    let last = LAST_SIGINT_MS.swap(now_ms, Ordering::Relaxed);
    if now_ms - last <= SIGINT_EXIT_WINDOW_MS {
        let pid = SHELL_CHILD_PID.load(Ordering::Relaxed);
        if pid > 0 {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    } else {
        let hint = b"\n(ctrl+c again within 2s exits the shell)\n";
        unsafe {
            libc::write(2, hint.as_ptr().cast(), hint.len());
        }
    }
}

struct ShellSignalGuard {
    previous: libc::sigaction,
}

impl ShellSignalGuard {
    fn install() -> Self {
        use std::sync::atomic::Ordering;
        LAST_SIGINT_MS.store(i64::MIN / 2, Ordering::Relaxed);
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = shell_sigint as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&raw mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, &raw const action, &raw mut previous);
            ShellSignalGuard { previous }
        }
    }
}

impl Drop for ShellSignalGuard {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        SHELL_CHILD_PID.store(0, Ordering::Relaxed);
        unsafe {
            libc::sigaction(libc::SIGINT, &raw const self.previous, std::ptr::null_mut());
        }
    }
}

fn wait_interactive(mut child: std::process::Child) -> std::io::Result<ExitStatus> {
    use std::sync::atomic::Ordering;
    SHELL_CHILD_PID.store(child.id() as i32, Ordering::Relaxed);
    child.wait()
}

pub fn shell(env: &Environment, project: Option<&Project>, windows: bool) -> Result<ExitStatus> {
    let _session = SHELL_SESSION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _guard = ShellSignalGuard::install();
    if windows {
        if let Some(p) = project {
            crate::envops::link_project(env, p)?;
        }
        let (program, args) =
            shell_invocation(has_powershell(env), dos_cwd(env, project).as_deref());
        let request = ExecutionRequest {
            program: program.clone(),
            args,
            prefix: env.layout.prefix(),
            cwd: windows_cwd(env, project),
            env: windows_env(env, project),
            sandbox: None,
            display: lsw_runtime::DisplayMode::Inherit,
            emulate: crate::emulateops::resolve(env.manifest.target_arch)?,
        };
        if crate::ttyops::stdin_is_tty() {
            let command = WineRuntime.command(&request)?;
            return crate::ttyops::run_shell_in_pty(
                command,
                "\r\n(ctrl+c again within 2s exits the shell)\r\n",
            );
        }
        let child = WineRuntime.spawn(&request)?;
        return wait_interactive(child).map_err(|e| Error::io(program, e));
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let tc = &env.manifest.toolchain;
    let mut cmd = Command::new(&shell);
    cmd.env("LSW_ENV", &env.name)
        .env("WINEPREFIX", env.layout.prefix())
        .env("CC", &tc.cc)
        .env("CXX", &tc.cxx)
        .env("LSW_TARGET_FLAGS", tc.c_flags.join(" "));
    if let Some(p) = project {
        cmd.env("LSW_PROJECT", &p.root);
        cmd.current_dir(&p.root);
    }
    cmd.env("PS1", format!("(lsw:{}) \\w \\$ ", env.name));

    let child = cmd
        .spawn()
        .map_err(|e| Error::io(PathBuf::from(&shell), e))?;
    wait_interactive(child).map_err(|e| Error::io(PathBuf::from(shell), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_invocation_carries_no_untrusted_path() {
        let (prog, args) = shell_invocation(true, None);
        assert_eq!(prog, PathBuf::from("powershell.exe"));
        assert_eq!(
            args,
            vec!["-NoExit", "-Command", "Set-Location", "$env:USERPROFILE"]
        );

        let (prog, args) = shell_invocation(false, None);
        assert_eq!(prog, PathBuf::from("cmd.exe"));
        assert_eq!(args, vec!["/k", "cd", "/d", "%USERPROFILE%"]);
    }

    #[test]
    fn shell_invocation_changes_to_the_project_dos_dir() {
        let (_, args) = shell_invocation(false, Some("C:\\src\\demo"));
        assert_eq!(args, vec!["/k", "cd", "/d", "C:\\src\\demo"]);

        let (_, args) = shell_invocation(true, Some("C:\\src\\demo"));
        assert_eq!(
            args,
            vec![
                "-NoExit",
                "-Command",
                "Set-Location",
                "-LiteralPath",
                "C:\\src\\demo"
            ]
        );
    }

    #[test]
    fn dos_paths_with_shell_metachars_are_rejected() {
        assert!(safe_dos_path("C:\\src\\demo"));
        assert!(safe_dos_path("C:\\src\\my app-1.2_x+~"));
        for bad in [
            "C:\\src\\a&calc",
            "C:\\src\\a|b",
            "C:\\src\\a\"b",
            "C:\\src\\a$b",
            "C:\\src\\a`b",
            "C:\\src\\a;b",
            "C:\\src\\a(b)",
            "C:\\src\\a^b",
            "C:\\src\\a%b%",
        ] {
            assert!(!safe_dos_path(bad), "accepted {bad:?}");
        }
    }

    #[test]
    fn wine_builtin_stub_is_not_a_real_windows_binary() {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("powershell.exe");
        let mut image = vec![0u8; 128];
        image[..2].copy_from_slice(b"MZ");
        image[64..80].copy_from_slice(WINE_BUILTIN_MARKER);
        std::fs::write(&stub, &image).unwrap();
        assert!(!is_real_windows_binary(&stub));

        let real = dir.path().join("real.exe");
        image[64..80].copy_from_slice(&[0u8; 16]);
        std::fs::write(&real, &image).unwrap();
        assert!(is_real_windows_binary(&real));

        assert!(!is_real_windows_binary(&dir.path().join("missing.exe")));
    }

    #[test]
    fn msi_detection_and_z_drive_mapping() {
        assert!(is_msi(Path::new("/tmp/Setup.MSI")));
        assert!(is_msi(Path::new("/a/b/installer.msi")));
        assert!(!is_msi(Path::new("/a/b/app.exe")));
        assert_eq!(
            z_drive_path(Path::new("/home/u/x.msi")),
            "Z:\\home\\u\\x.msi"
        );
    }

    #[test]
    fn env_overrides_applies_vars_and_resolves_present_secrets_only() {
        let mut section = lsw_config::EnvSection::default();
        section.vars.insert("FOO".into(), "bar".into());
        section.secret.insert("TOKEN".into(), "HOST_TOKEN".into());
        section
            .secret
            .insert("ABSENT".into(), "HOST_MISSING".into());
        let lookup = |k: &str| match k {
            "HOST_TOKEN" => Some("s3cr3t".to_owned()),
            _ => None,
        };
        let out = env_overrides(&section, lookup);
        assert!(out.contains(&("FOO".into(), "bar".into())));
        assert!(out.contains(&("TOKEN".into(), "s3cr3t".into())));
        assert!(!out.iter().any(|(k, _)| k == "ABSENT"));
    }

    #[test]
    fn processor_architecture_maps_each_arch() {
        assert_eq!(processor_architecture(TargetArch::X86_64), "AMD64");
        assert_eq!(processor_architecture(TargetArch::X86), "x86");
        assert_eq!(processor_architecture(TargetArch::Aarch64), "ARM64");
        assert_eq!(processor_architecture(TargetArch::Arm64Ec), "ARM64");
        assert_eq!(processor_architecture(TargetArch::Armv7), "ARM");
    }

    #[test]
    fn missing_path_with_separator_is_rejected() {
        let err = resolve_program(Path::new("/nope/missing.exe"), Domain::Auto).unwrap_err();
        assert!(err.to_string().contains("LSW2004"));
    }

    #[test]
    fn windows_style_paths_pass_through_for_the_runtime() {
        let r = resolve_program(Path::new("C:\\windows\\system32\\cmd.exe"), Domain::Auto).unwrap();
        assert!(
            matches!(r, ResolvedProgram::RuntimeResolved(p) if p.to_str().unwrap().starts_with("C:"))
        );
    }

    #[test]
    fn bare_names_resolve_via_path_for_host_execution() {
        let r = resolve_program(Path::new("sh"), Domain::Host).unwrap();
        match r {
            ResolvedProgram::HostPath(p) => {
                assert!(p.is_absolute());
                assert!(p.ends_with("sh"));
            }
            ResolvedProgram::RuntimeResolved(_) => panic!("sh must resolve on PATH"),
        }
    }

    #[test]
    fn unknown_bare_name_passes_through_only_for_windows_domain() {
        let r = resolve_program(Path::new("cmd.exe"), Domain::Windows).unwrap();
        assert!(matches!(r, ResolvedProgram::RuntimeResolved(_)));

        let err = resolve_program(Path::new("no-such-tool-xyz"), Domain::Auto).unwrap_err();
        assert!(err.to_string().contains("--windows"));
    }

    #[test]
    fn relative_existing_file_is_absolutized() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("prog");
        std::fs::write(&file, b"#!/bin/sh\n").unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let r = resolve_program(Path::new("prog"), Domain::Host);
        std::env::set_current_dir(prev).unwrap();
        match r.unwrap() {
            ResolvedProgram::HostPath(p) => assert!(p.is_absolute()),
            ResolvedProgram::RuntimeResolved(_) => panic!("existing file must be a host path"),
        }
    }
}
