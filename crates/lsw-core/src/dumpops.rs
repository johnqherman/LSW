use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use minidump::{Minidump, MinidumpException, MinidumpModuleList, MinidumpSystemInfo, Module};

use crate::envops::Environment;
use crate::error::{Error, Result};

const DUMP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct DumpSummary {
    pub reason: String,
    pub crash_address: u64,
    pub instruction_pointer: Option<u64>,
    pub faulting_module: Option<String>,
    pub faulting_offset: Option<u64>,
    pub crashing_thread: Option<u32>,
    pub os: String,
    pub cpu: String,
    pub module_count: usize,
}

pub fn analyze(path: &Path) -> Result<DumpSummary> {
    let dump = Minidump::read_path(path).map_err(|e| dump_err(path, &e))?;
    let system: MinidumpSystemInfo = dump.get_stream().map_err(|e| dump_err(path, &e))?;
    let exception: MinidumpException = dump.get_stream().map_err(|e| dump_err(path, &e))?;
    let modules: MinidumpModuleList = dump.get_stream().unwrap_or_default();

    let reason = exception
        .get_crash_reason(system.os, system.cpu)
        .to_string();
    let crash_address = exception.get_crash_address(system.os, system.cpu);
    let crashing_thread = Some(exception.get_crashing_thread_id());

    let misc = dump.get_stream().ok();
    let context = exception.context(&system, misc.as_ref());
    let instruction_pointer = context.as_ref().map(|c| c.get_instruction_pointer());

    let located = instruction_pointer
        .and_then(|ip| modules.module_at_address(ip).map(|m| (m, ip)))
        .or_else(|| {
            modules
                .module_at_address(crash_address)
                .map(|m| (m, crash_address))
        });
    let (faulting_module, faulting_offset) = match located {
        Some((module, addr)) => (
            Some(basename(&module.code_file())),
            Some(addr - module.base_address()),
        ),
        None => (None, None),
    };

    let module_count = modules.iter().count();

    Ok(DumpSummary {
        reason,
        crash_address,
        instruction_pointer,
        faulting_module,
        faulting_offset,
        crashing_thread,
        os: format!("{:?}", system.os),
        cpu: format!("{:?}", system.cpu),
        module_count,
    })
}

pub fn dump_path_for(pe: &Path) -> PathBuf {
    let name = pe.file_name().map_or_else(
        || "program".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    pe.with_file_name(format!("{name}.dmp"))
}

pub fn capture_wine_dump(
    env: &Environment,
    program: &Path,
    args: &[String],
    out: &Path,
    break_immediately: bool,
) -> Result<bool> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if !program.is_file() {
        return Err(Error::NotExecutable {
            program: program.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let program = std::path::absolute(program).map_err(|e| Error::io(program.to_path_buf(), e))?;
    let winedbg = crate::buildops::which("winedbg").ok_or_else(|| Error::ToolMissing {
        tool: "winedbg".into(),
        fix: "install wine (winedbg ships with it)".into(),
    })?;
    let out_abs = std::path::absolute(out).map_err(|e| Error::io(out.to_path_buf(), e))?;
    if std::fs::symlink_metadata(&out_abs).is_ok() {
        std::fs::remove_file(&out_abs).map_err(|e| Error::io(out_abs.clone(), e))?;
    }
    let windows_out = crate::runops::z_drive_path(&out_abs);
    let script = if break_immediately {
        format!("minidump \"{windows_out}\"\nquit\n")
    } else {
        format!("cont\nminidump \"{windows_out}\"\nquit\n")
    };

    let mut command = Command::new(&winedbg);
    lsw_runtime::scrub_wine_env(&mut command);
    command
        .arg(&program)
        .args(args)
        .env("WINEPREFIX", env.layout.prefix())
        .env("WINEDEBUG", "fixme-all")
        .env("WINEDLLOVERRIDES", "winemenubuilder.exe=d")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|e| Error::io(winedbg.clone(), e))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(script.as_bytes());
    }
    let deadline = Instant::now() + DUMP_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    Ok(out_abs.is_file())
}

fn basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

fn dump_err(path: &Path, e: &dyn std::fmt::Display) -> Error {
    Error::DumpParse {
        path: path.to_path_buf(),
        detail: e.to_string(),
    }
}
