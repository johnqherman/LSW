use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::project::Project;

use super::{default_remote_dir, expand_tilde, ssh_opts, validate_windows_dir, which};

#[derive(Debug, Serialize)]
pub struct DllProbe {
    pub name: String,
    pub loaded: bool,
    pub missing_functions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportProbe {
    pub host: String,
    pub dlls: Vec<DllProbe>,
}

pub fn probe_imports(project: &Project, program: &std::path::Path) -> Result<Option<ImportProbe>> {
    let cfg = &project.manifest.verify;
    let Some(host) = cfg.host.clone() else {
        return Ok(None);
    };
    let imports = lsw_pe::imported_symbols(program).unwrap_or_default();
    let transport = cfg.transport.as_deref().unwrap_or("ssh");
    if transport != "ssh" {
        return Ok(None);
    }
    super::validate_ssh_host(&host)?;
    if which("ssh").is_none() {
        return Err(Error::ToolMissing {
            tool: "ssh".into(),
            fix: "install openssh-client to reach the Windows verification host".into(),
        });
    }

    let mut grouped: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (dll, func) in imports {
        let dll = dll.to_ascii_lowercase();
        if dll.is_empty() || !dll.chars().all(|c| c.is_ascii_graphic() && c != '\'') {
            continue;
        }
        let clean = func.trim();
        if clean.is_empty()
            || clean.starts_with('#')
            || !clean.chars().all(|c| c.is_ascii_graphic() && c != '\'')
        {
            continue;
        }
        let entry = grouped.entry(dll).or_default();
        if entry.len() < 256 && !entry.iter().any(|f| f == clean) {
            entry.push(clean.to_owned());
        }
    }
    if grouped.is_empty() {
        return Ok(Some(ImportProbe {
            host,
            dlls: Vec::new(),
        }));
    }

    let identity = cfg.identity_file.as_deref().map(expand_tilde);
    let remote_dir = cfg
        .remote_dir
        .clone()
        .unwrap_or_else(|| default_remote_dir(project));
    validate_windows_dir(&remote_dir)?;
    let remote_fwd = remote_dir.replace('\\', "/");
    let remote_script = format!("{remote_dir}\\lsw_probe.ps1");
    let script = probe_script(&grouped);

    let mkdir = Command::new("ssh")
        .args(ssh_opts(identity.as_deref()))
        .arg(&host)
        .arg(format!(
            "cmd /c \"if not exist \"{remote_dir}\" mkdir \"{remote_dir}\"\""
        ))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    if !mkdir.status.success() {
        return Err(Error::ProbeFailed {
            host,
            detail: String::from_utf8_lossy(&mkdir.stderr).trim().to_owned(),
        });
    }

    let local_script =
        std::env::temp_dir().join(format!("lsw_probe_{}.ps1", project.manifest.project.name));
    std::fs::write(&local_script, script.as_bytes())
        .map_err(|e| Error::io(local_script.clone(), e))?;
    let scp = Command::new("scp")
        .args(ssh_opts(identity.as_deref()))
        .arg(&local_script)
        .arg(format!("{host}:{remote_fwd}/lsw_probe.ps1"))
        .output()
        .map_err(|e| Error::io(PathBuf::from("scp"), e))?;
    let _ = std::fs::remove_file(&local_script);
    if !scp.status.success() {
        return Err(Error::ProbeFailed {
            host,
            detail: String::from_utf8_lossy(&scp.stderr).trim().to_owned(),
        });
    }

    let out = Command::new("ssh")
        .args(ssh_opts(identity.as_deref()))
        .arg(&host)
        .arg(format!(
            "powershell -NoProfile -ExecutionPolicy Bypass -File \"{remote_script}\""
        ))
        .output()
        .map_err(|e| Error::io(PathBuf::from("ssh"), e))?;
    if !out.status.success() {
        return Err(Error::ProbeFailed {
            host,
            detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut dlls: std::collections::BTreeMap<String, DllProbe> = grouped
        .keys()
        .map(|name| {
            (
                name.clone(),
                DllProbe {
                    name: name.clone(),
                    loaded: false,
                    missing_functions: Vec::new(),
                },
            )
        })
        .collect();
    for line in stdout.lines() {
        let cols: Vec<&str> = line.trim().split('\t').collect();
        match cols.as_slice() {
            ["DLL", name, status] => {
                if let Some(d) = dlls.get_mut(&name.to_ascii_lowercase()) {
                    d.loaded = *status == "OK";
                }
            }
            ["FN", name, func, "MISSING"] => {
                if let Some(d) = dlls.get_mut(&name.to_ascii_lowercase()) {
                    d.missing_functions.push((*func).to_owned());
                }
            }
            _ => {}
        }
    }

    Ok(Some(ImportProbe {
        host,
        dlls: dlls.into_values().collect(),
    }))
}

fn ps_dquote(s: &str) -> String {
    s.replace('`', "``").replace('$', "`$").replace('"', "`\"")
}

fn probe_script(grouped: &std::collections::BTreeMap<String, Vec<String>>) -> String {
    let mut s = String::from(
        "$ErrorActionPreference='SilentlyContinue'\n\
         Add-Type @\"\n\
         using System;\n\
         using System.Runtime.InteropServices;\n\
         public static class LswProbe {\n\
         [DllImport(\"kernel32\", SetLastError=true, CharSet=CharSet.Unicode)] public static extern IntPtr LoadLibraryW(string n);\n\
         [DllImport(\"kernel32\", SetLastError=true, CharSet=CharSet.Ansi)] public static extern IntPtr GetProcAddress(IntPtr h, string p);\n\
         }\n\
         \"@\n",
    );
    for (dll, funcs) in grouped {
        let dll_d = ps_dquote(dll);
        s.push_str(&format!("$h=[LswProbe]::LoadLibraryW('{dll}')\n"));
        s.push_str(&format!(
            "if($h -eq [IntPtr]::Zero){{Write-Output \"DLL`t{dll_d}`tMISSING\"}}else{{Write-Output \"DLL`t{dll_d}`tOK\"\n"
        ));
        for func in funcs {
            let func_d = ps_dquote(func);
            s.push_str(&format!(
                "if([LswProbe]::GetProcAddress($h,'{func}') -eq [IntPtr]::Zero){{Write-Output \"FN`t{dll_d}`t{func_d}`tMISSING\"}}\n"
            ));
        }
        s.push_str("}\n");
    }
    s
}
