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

pub(crate) fn crash(
    file: &Path,
    force: bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let file = if force {
        let (_p, env) = active_env(dirs)?;
        let dump = lsw_core::dumpops::dump_path_for(file);
        let written = lsw_core::dumpops::capture_wine_dump(&env, file, &[], &dump, true)?;
        if !written {
            eprintln!(
                "error: winedbg did not produce a dump for {}",
                file.display()
            );
            return Ok(ExitCode::FAILURE);
        }
        eprintln!("[lsw] dump written to {}", dump.display());
        dump
    } else {
        file.to_path_buf()
    };
    let s = lsw_core::dumpops::analyze(&file)?;
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
        crate::cmd::emit_json(&report);
    } else {
        println!("\n{}  {}\n", color::bold("LSW AUDIT"), file.display());
        for c in &report.checks {
            let mark = match c.status {
                lsw_core::auditops::AuditStatus::Enabled => color::green("+"),
                lsw_core::auditops::AuditStatus::Disabled => color::red("X"),
                lsw_core::auditops::AuditStatus::NotApplicable => color::dim("-"),
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
        crate::cmd::emit_json(&names);
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
    crate::cmd::emit_json(&bom);
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn diff(a: &Path, b: &Path, format: Format) -> lsw_core::Result<ExitCode> {
    let report = lsw_core::diffops::diff(a, b)?;
    if format == Format::Json {
        crate::cmd::emit_json(&report);
    } else {
        let mut any = false;
        for (label, d) in [
            ("imports", &report.imports),
            ("exports", &report.exports),
            ("section", &report.sections),
        ] {
            for x in &d.added {
                println!("+ {label} {x}");
                any = true;
            }
            for x in &d.removed {
                println!("- {label} {x}");
                any = true;
            }
        }
        for r in &report.resized {
            println!("~ section {} {:+} bytes", r.name, r.raw_size_delta);
            any = true;
        }
        if report.size_delta != 0 {
            println!("~ file size {:+} bytes", report.size_delta);
            any = true;
        }
        if !any {
            println!(
                "no import/export/section differences (this compares the PE surface, not bytes)"
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn size(
    file: &Path,
    baseline: &Option<std::path::PathBuf>,
    max_growth: &Option<f64>,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let report = lsw_core::sizeops::size(file, baseline.as_deref(), *max_growth)?;
    if format == Format::Json {
        crate::cmd::emit_json(&report);
    } else {
        println!(
            "\nLSW SIZE  {}  ({} bytes)\n",
            report.file, report.file_size
        );
        let with_baseline = report.baseline.is_some();
        if with_baseline {
            println!(
                "{:<14} {:>12} {:>7}  {:>12} {:>10}",
                "bucket", "bytes", "%", "baseline", "delta"
            );
        } else {
            println!("{:<14} {:>12} {:>7}", "bucket", "bytes", "%");
        }
        for b in &report.buckets {
            if with_baseline {
                let growth = b
                    .growth_percent
                    .map(|g| format!(" ({g:+.1}%)"))
                    .unwrap_or_default();
                println!(
                    "{:<14} {:>12} {:>6.1}%  {:>12} {:>+10}{growth}",
                    b.name,
                    b.bytes,
                    b.percent,
                    b.baseline_bytes.unwrap_or(0),
                    b.delta.unwrap_or(0)
                );
            } else {
                println!("{:<14} {:>12} {:>6.1}%", b.name, b.bytes, b.percent);
            }
        }
        if let Some(base_size) = report.baseline_size {
            println!(
                "\nfile size: {} -> {} ({:+} bytes)",
                base_size,
                report.file_size,
                report.file_size as i64 - base_size as i64
            );
        }
        if !report.exceeded.is_empty() {
            println!(
                "\n{} bucket(s) grew beyond the --max-growth limit: {}",
                color::red("FAIL:"),
                report.exceeded.join(", ")
            );
        }
    }
    crate::cmd::exit_ok(report.exceeded.is_empty())
}

pub(crate) fn strings(file: &Path, min: &usize, format: Format) -> lsw_core::Result<ExitCode> {
    let found = lsw_core::stringsops::strings(file, *min)?;
    if format == Format::Json {
        crate::cmd::emit_json(&found);
    } else {
        for s in &found {
            println!("{s}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn deps(op: &DepsCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        DepsCmd::Tree { file } => {
            let Some(file) = crate::cmd::resolve_pe(file, dirs)? else {
                return Ok(ExitCode::FAILURE);
            };
            let env = active_env(dirs).ok().map(|(_, e)| e);
            let root = lsw_core::depsops::tree(env.as_ref(), &file)?;
            if format == Format::Json {
                crate::cmd::emit_json(&root);
            } else {
                print_dep_tree(&root, 0);
            }
            Ok(ExitCode::SUCCESS)
        }

        DepsCmd::Add { name } => {
            let (p, env) = active_env(dirs)?;
            let pkg = lsw_core::depsops::add(&p, env.manifest.target_arch, dirs, name)?;
            if format == Format::Json {
                crate::cmd::emit_json(&pkg);
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
            let (p, env) = active_env(dirs)?;
            let removed = lsw_core::depsops::remove(&p, env.manifest.target_arch, name)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::json!({ "name": name, "removed": removed })
                );
            } else if removed {
                println!("{} removed {name}", color::yellow("-"));
            } else {
                eprintln!("error: {name} is not an installed dependency");
            }
            crate::cmd::exit_ok(removed)
        }

        DepsCmd::List => {
            let (p, _env) = active_env(dirs)?;
            let deps = lsw_core::depsops::list(&p);
            if format == Format::Json {
                crate::cmd::emit_json(&deps);
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
