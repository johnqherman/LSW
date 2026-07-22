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

fn sandbox_spec(
    env: &Environment,
    project: Option<&Project>,
    sandbox: Sandbox,
) -> Result<Option<lsw_runtime::SandboxSpec>> {
    match sandbox {
        Sandbox::None => Ok(None),
        Sandbox::Strict => {
            let mut rw_binds = vec![env.layout.root.clone()];
            if let Some(p) = project {
                rw_binds.push(p.root.clone());
            }
            let network = match project {
                Some(p) => parse_network(&p.manifest.sandbox.network).ok_or_else(|| {
                    Error::InvalidSandboxNetwork {
                        value: p.manifest.sandbox.network.clone(),
                    }
                })?,
                None => lsw_runtime::NetworkMode::Host,
            };
            let (cpu_seconds, memory_bytes) = project
                .map(|p| {
                    (
                        p.manifest.sandbox.cpu_seconds,
                        p.manifest.sandbox.memory_mb.map(|mb| mb * 1024 * 1024),
                    )
                })
                .unwrap_or((None, None));
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

    if domain != Domain::Host
        && let ResolvedProgram::HostPath(p) = &resolved
        && is_msi(p)
    {
        if let Some(pr) = project {
            buildops::check_lock(pr, env)?;
        }
        let mut msi_args = vec!["/i".to_owned(), z_drive_path(p)];
        msi_args.extend(args.iter().cloned());
        let status = WineRuntime.execute(&ExecutionRequest {
            program: PathBuf::from("msiexec"),
            args: msi_args,
            prefix: env.layout.prefix(),
            cwd: windows_cwd(env, project),
            env: windows_env(env, project),
            sandbox: None,
            display: lsw_runtime::DisplayMode::Inherit,
            emulate: crate::emulateops::resolve(env.manifest.target_arch)?,
        })?;
        return Ok(RunReport {
            domain: Domain::Windows,
            status,
        });
    }

    let mut is_gui: Option<bool> = None;

    let (chosen, launch) = match resolved {
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
                d => d,
            };
            (chosen, p)
        }
    };

    let status = match chosen {
        Domain::Windows => {
            if let Some(p) = project {
                buildops::check_lock(p, env)?;
            }
            WineRuntime.execute(&ExecutionRequest {
                program: launch,
                args: args.to_vec(),
                prefix: env.layout.prefix(),
                cwd: windows_cwd(env, project),
                env: windows_env(env, project),
                sandbox: sandbox_spec(env, project, sandbox)?,
                display: display_mode(display, is_gui),
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

pub const WINDOWS_USER: &str = "lsw";

fn is_msi(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"))
}

fn z_drive_path(path: &Path) -> String {
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

fn windows_env(env: &Environment, project: Option<&Project>) -> Vec<(String, String)> {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let profile = format!("C:\\users\\{WINDOWS_USER}");
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
        ("USERNAME".into(), WINDOWS_USER.into()),
        ("USERPROFILE".into(), profile.clone()),
        ("HOMEDRIVE".into(), "C:".into()),
        ("HOMEPATH".into(), format!("\\users\\{WINDOWS_USER}")),
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
    let cwd = std::env::current_dir().ok()?;
    let mapper = crate::envops::mapper(env, project);
    let windows = mapper.to_windows(&cwd).ok()?;
    let rest = windows.strip_prefix("C:\\")?;
    if rest.is_empty() {
        return Some(env.layout.drive_c());
    }
    Some(env.layout.drive_c().join(rest.replace('\\', "/")))
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
        let absolute =
            std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;
        return Ok(ResolvedProgram::HostPath(absolute));
    }

    if let Some(found) = buildops::which(&text) {
        let absolute = std::path::absolute(&found).map_err(|e| Error::io(found.clone(), e))?;
        return Ok(ResolvedProgram::HostPath(absolute));
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

fn shell_invocation(powershell: bool, dos: Option<&str>) -> (PathBuf, Vec<String>) {
    if powershell {
        let mut args = vec!["-NoExit".to_owned()];
        if let Some(dos) = dos {
            args.push("-Command".to_owned());
            args.push(format!("Set-Location -LiteralPath '{dos}'"));
        }
        (PathBuf::from("powershell.exe"), args)
    } else {
        let args = dos
            .map(|dos| vec!["/k".to_owned(), format!("cd /d {dos}")])
            .unwrap_or_default();
        (PathBuf::from("cmd.exe"), args)
    }
}

fn has_powershell(env: &Environment) -> bool {
    env.layout
        .drive_c()
        .join("windows/system32/WindowsPowerShell/v1.0/powershell.exe")
        .is_file()
}

pub fn shell(env: &Environment, project: Option<&Project>, windows: bool) -> Result<ExitStatus> {
    if windows {
        let dos = project.and_then(|p| crate::envops::mapper(env, p).to_windows(&p.root).ok());
        let (program, args) = shell_invocation(has_powershell(env), dos.as_deref());
        return Ok(WineRuntime.execute(&ExecutionRequest {
            program,
            args,
            prefix: env.layout.prefix(),
            cwd: windows_cwd(env, project),
            env: windows_env(env, project),
            sandbox: None,
            display: lsw_runtime::DisplayMode::Inherit,
            emulate: crate::emulateops::resolve(env.manifest.target_arch)?,
        })?);
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

    cmd.status().map_err(|e| Error::io(PathBuf::from(shell), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_invocation_prefers_powershell_when_present() {
        let (prog, args) = shell_invocation(true, Some("C:\\src\\demo"));
        assert_eq!(prog, PathBuf::from("powershell.exe"));
        assert_eq!(args[0], "-NoExit");
        assert!(args.last().unwrap().contains("Set-Location"));
        assert!(args.last().unwrap().contains("C:\\src\\demo"));

        let (prog, args) = shell_invocation(false, Some("C:\\src\\demo"));
        assert_eq!(prog, PathBuf::from("cmd.exe"));
        assert_eq!(args, vec!["/k", "cd /d C:\\src\\demo"]);

        let (_, args) = shell_invocation(false, None);
        assert!(args.is_empty());
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
