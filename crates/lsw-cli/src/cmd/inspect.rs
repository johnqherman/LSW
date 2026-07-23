use std::path::Path;
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::{DepsCmd, Format};
use crate::{active_env, color, print_dep_tree};

pub(crate) fn inspect(file: &Path, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    let env = active_env(dirs).ok().map(|(_, e)| e);
    let report = lsw_core::inspect(file, env.as_ref())?;
    if format == Format::Json {
        let imports: Vec<_> = report
            .imports
            .iter()
            .map(|i| serde_json::json!({ "dll": i.dll, "available": i.available }))
            .collect();
        let sections: Vec<_> = report
            .details
            .sections
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "virtual_size": s.virtual_size,
                    "raw_size": s.raw_size,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "format": format!("{:?}", report.info.format),
                "machine": format!("{:?}", report.info.machine),
                "subsystem": format!("{:?}", report.info.subsystem),
                "entry_point": report.details.entry_point,
                "image_base": report.details.image_base,
                "sections": sections,
                "resources": {
                    "has_manifest": report.resources.manifest.is_some(),
                    "execution_level": report.resources.execution_level,
                    "dpi_aware": report.resources.dpi_aware,
                    "version": report.resources.version,
                    "has_icon": report.resources.has_icon,
                },
                "imports": imports,
            })
        );
    } else {
        println!("Format:      {:?}", report.info.format);
        println!("Machine:     {:?}", report.info.machine);
        println!("Subsystem:   {:?}", report.info.subsystem);
        println!("Entry point: 0x{:08x}", report.details.entry_point);
        println!("Image base:  0x{:x}", report.details.image_base);
        let h = &report.hardening;
        let flag = |b: bool| if b { "yes" } else { "no" };
        println!(
            "Hardening:   ASLR={} DEP={} CFG={} signed={}",
            flag(h.aslr),
            flag(h.dep),
            flag(h.cfg),
            flag(h.signed)
        );
        println!("Sections:");
        for s in &report.details.sections {
            println!(
                "  {:<10} vsize={:<10} raw={}",
                s.name, s.virtual_size, s.raw_size
            );
        }
        let res = &report.resources;
        if res.manifest.is_some() || res.has_icon || !res.version.is_empty() {
            println!("Resources:");
            if let Some(level) = &res.execution_level {
                println!("  manifest execution-level: {level}");
            }
            if let Some(dpi) = &res.dpi_aware {
                println!("  manifest dpi-aware: {dpi}");
            }
            for (k, v) in &res.version {
                println!("  {k}: {v}");
            }
            println!("  icon: {}", flag(res.has_icon));
        }
        println!("Imports:");
        for i in &report.imports {
            let availability = match i.available {
                Some(true) => "available",
                Some(false) => "MISSING in runtime",
                None => "unknown (no environment)",
            };
            println!("  {:<24} {}", i.dll, availability);
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn crash(file: &Path, format: Format) -> lsw_core::Result<ExitCode> {
    let s = lsw_core::dumpops::analyze(file)?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::json!({
                "reason": s.reason,
                "crash_address": s.crash_address,
                "instruction_pointer": s.instruction_pointer,
                "faulting_module": s.faulting_module,
                "faulting_offset": s.faulting_offset,
                "crashing_thread": s.crashing_thread,
                "os": s.os,
                "cpu": s.cpu,
                "module_count": s.module_count,
            })
        );
    } else {
        println!("Exception:   {}", s.reason);
        println!("Address:     {:#x}", s.crash_address);
        match (&s.faulting_module, s.faulting_offset) {
            (Some(m), Some(off)) => println!("Faulting:    {m}+{off:#x}"),
            _ => println!("Faulting:    unknown (no module for instruction pointer)"),
        }
        if let Some(ip) = s.instruction_pointer {
            println!("Instruction: {ip:#x}");
        }
        if let Some(tid) = s.crashing_thread {
            println!("Thread:      {tid}");
        }
        println!("Platform:    {} {}", s.os, s.cpu);
        println!("Modules:     {}", s.module_count);
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn audit(file: &Path, format: Format) -> lsw_core::Result<ExitCode> {
    let report = lsw_core::auditops::audit(file)?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("serializes")
        );
    } else {
        println!("\n{}  {}\n", color::bold("LSW AUDIT"), file.display());
        for c in &report.checks {
            let mark = if c.enabled {
                color::green("+")
            } else {
                color::red("X")
            };
            println!("  {mark} {:<22} {}", c.name, c.detail);
        }
        println!(
            "\n{}",
            if report.hardened {
                color::green("baseline hardening present (ASLR + DEP)")
            } else {
                color::red("WEAK: missing ASLR or DEP")
            }
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn exports(file: &Path, format: Format) -> lsw_core::Result<ExitCode> {
    let names = lsw_core::auditops::exports(file)?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&names).expect("serializes")
        );
    } else if names.is_empty() {
        println!("no exports (not a DLL, or no export table)");
    } else {
        for n in &names {
            println!("{n}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn sbom(file: &Path) -> lsw_core::Result<ExitCode> {
    let bom = lsw_core::sbomops::sbom(file)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&bom).expect("serializes")
    );
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn diff(a: &Path, b: &Path, format: Format) -> lsw_core::Result<ExitCode> {
    let report = lsw_core::diffops::diff(a, b)?;
    if format == Format::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("serializes")
        );
    } else {
        let mut any = false;
        for (label, d) in [("imports", &report.imports), ("exports", &report.exports)] {
            for x in &d.added {
                println!("+ {label} {x}");
                any = true;
            }
            for x in &d.removed {
                println!("- {label} {x}");
                any = true;
            }
        }
        if !any {
            println!("no import/export differences (this compares the PE API surface, not bytes)");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn strings(file: &Path, min: &usize) -> lsw_core::Result<ExitCode> {
    for s in lsw_core::stringsops::strings(file, *min)? {
        println!("{s}");
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn deps(op: &DepsCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        DepsCmd::Tree { file } => {
            let env = active_env(dirs).ok().map(|(_, e)| e);
            let root = lsw_core::depsops::tree(env.as_ref(), file)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&root).expect("serializes")
                );
            } else {
                print_dep_tree(&root, 0);
            }
            Ok(ExitCode::SUCCESS)
        }

        DepsCmd::Add { name } => {
            let (p, _env) = active_env(dirs)?;
            let pkg = lsw_core::depsops::add(&p, dirs, name)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&pkg).expect("serializes")
                );
            } else {
                println!(
                    "{} added {} {}",
                    color::green("+"),
                    pkg.name,
                    color::dim(&pkg.version)
                );
                println!("  headers and libraries under deps/; recorded in lsw.toml");
            }
            Ok(ExitCode::SUCCESS)
        }

        DepsCmd::Remove { name } => {
            let (p, _env) = active_env(dirs)?;
            if lsw_core::depsops::remove(&p, name)? {
                println!("{} removed {name}", color::yellow("-"));
            } else {
                println!("{name} is not an installed dependency");
            }
            Ok(ExitCode::SUCCESS)
        }

        DepsCmd::List => {
            let (p, _env) = active_env(dirs)?;
            let deps = lsw_core::depsops::list(&p);
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&deps).expect("serializes")
                );
            } else if deps.is_empty() {
                println!("no dependencies (add one with: lsw deps add <name>)");
            } else {
                for d in &deps {
                    println!("  {:<20} {}", d.name, d.version);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
