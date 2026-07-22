use std::path::Path;

use minidump::{Minidump, MinidumpException, MinidumpModuleList, MinidumpSystemInfo, Module};

use crate::error::{Error, Result};

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

fn basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

fn dump_err(path: &Path, e: &dyn std::fmt::Display) -> Error {
    Error::DumpParse {
        path: path.to_path_buf(),
        detail: e.to_string(),
    }
}
