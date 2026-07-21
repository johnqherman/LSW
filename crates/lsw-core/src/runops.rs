use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

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

pub fn run(
    env: &Environment,
    project: Option<&Project>,
    program: &Path,
    args: &[String],
    domain: Domain,
) -> Result<RunReport> {
    let resolved = resolve_program(program, domain)?;

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
                    BinaryKind::Pe(_) => Domain::Windows,
                    BinaryKind::Elf | BinaryKind::Script => Domain::Host,
                    BinaryKind::Unknown => {
                        return Err(Error::NotExecutable {
                            program: p,
                            detail: "not a PE, ELF, or script; pass --host or --windows to force"
                                .into(),
                        });
                    }
                },
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
                env: windows_env(env),
            })?
        }
        Domain::Host | Domain::Auto => Command::new(&launch)
            .args(args)
            .status()
            .map_err(|e| Error::io(launch.clone(), e))?,
    };

    Ok(RunReport {
        domain: chosen,
        status,
    })
}

fn windows_env(_env: &Environment) -> Vec<(String, String)> {
    vec![
        ("TEMP".into(), "C:\\Temp".into()),
        ("TMP".into(), "C:\\Temp".into()),
    ]
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

pub fn shell(env: &Environment, project: Option<&Project>, windows: bool) -> Result<ExitStatus> {
    if windows {
        return Ok(WineRuntime.execute(&ExecutionRequest {
            program: PathBuf::from("cmd.exe"),
            args: Vec::new(),
            prefix: env.layout.prefix(),
            cwd: windows_cwd(env, project),
            env: windows_env(env),
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
