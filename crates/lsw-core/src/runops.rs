use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use lsw_pe::BinaryKind;
use lsw_runtime::{ExecutionRequest, RuntimeProvider, WineRuntime};

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

pub fn run(
    env: &Environment,
    program: &Path,
    args: &[String],
    domain: Domain,
) -> Result<RunReport> {
    let resolved = resolve_program(program)?;
    let chosen = match domain {
        Domain::Auto => match lsw_pe::detect(&resolved)? {
            BinaryKind::Pe(_) => Domain::Windows,
            BinaryKind::Elf | BinaryKind::Script => Domain::Host,
            BinaryKind::Unknown => {
                return Err(Error::NotExecutable {
                    program: resolved,
                    detail: "not a PE, ELF, or script; pass --host or --windows to force".into(),
                });
            }
        },
        d => d,
    };

    let status = match chosen {
        Domain::Windows => WineRuntime.execute(&ExecutionRequest {
            program: resolved,
            args: args.to_vec(),
            prefix: env.layout.prefix(),
            cwd: None,
            env: windows_env(env),
        })?,
        Domain::Host | Domain::Auto => Command::new(&resolved)
            .args(args)
            .status()
            .map_err(|e| Error::io(resolved.clone(), e))?,
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

fn resolve_program(program: &Path) -> Result<PathBuf> {
    if program.is_file() {
        return Ok(program.to_path_buf());
    }
    let text = program.to_string_lossy();
    if text.len() >= 2 && text.as_bytes()[1] == b':' {
        return Ok(program.to_path_buf());
    }
    Err(Error::NotExecutable {
        program: program.to_path_buf(),
        detail: "file not found".into(),
    })
}

pub fn shell(env: &Environment, project: Option<&Project>, windows: bool) -> Result<ExitStatus> {
    if windows {
        return Ok(WineRuntime.execute(&ExecutionRequest {
            program: PathBuf::from("cmd.exe"),
            args: Vec::new(),
            prefix: env.layout.prefix(),
            cwd: project.map(|p| p.root.clone()),
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

    cmd.status()
        .map_err(|e| Error::io(PathBuf::from(shell), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_program_rejects_missing_host_path() {
        let err = resolve_program(Path::new("/nope/missing.exe")).unwrap_err();
        assert!(err.to_string().contains("LSW2004"));
    }

    #[test]
    fn resolve_program_passes_windows_style_paths_through() {
        let p = resolve_program(Path::new("C:\\windows\\system32\\cmd.exe")).unwrap();
        assert_eq!(p, PathBuf::from("C:\\windows\\system32\\cmd.exe"));
    }
}
