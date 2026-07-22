use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::{DotnetCmd, Format, RustCmd, SdkCmd};
use crate::{active_env, cwd};

pub(crate) fn rust(op: &RustCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        RustCmd::Init { name } => {
            let report = lsw_core::rustops::init(&cwd(), name.as_deref())?;
            println!("Initialized LSW Rust project at {}", report.root.display());
            for f in &report.created {
                println!("  created {}", f.display());
            }
            println!("Next: lsw env create <name> && lsw build");
            Ok(ExitCode::SUCCESS)
        }

        RustCmd::Doctor => {
            let (_p, env) = active_env(dirs)?;
            let report = lsw_core::rustops::doctor(&env)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                let mark = |c: lsw_core::rustops::Check| match c {
                    lsw_core::rustops::Check::Ok => "OK",
                    lsw_core::rustops::Check::NotConfigured => "NOT CONFIGURED",
                    lsw_core::rustops::Check::Missing => "MISSING",
                };
                println!("LSW Rust Doctor  (target {})\n", report.target);
                println!("  Compiler target   {}", mark(report.compiler_target));
                println!("  Linker            {}", mark(report.linker));
                println!("  CRT               {}", mark(report.crt));
                println!("  Windows imports   {}", mark(report.windows_imports));
                println!("  Runtime execution {}", mark(report.runtime_execution));
                println!("  Native validation {}", mark(report.native_validation));
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn dotnet(op: &DotnetCmd, dirs: &Dirs, format: Format) -> lsw_core::Result<ExitCode> {
    match op {
        DotnetCmd::Init { name } => {
            let report = lsw_core::dotnetops::init(&cwd(), name.as_deref())?;
            println!("Initialized LSW C# project at {}", report.root.display());
            for f in &report.created {
                println!("  created {}", f.display());
            }
            println!("Next: lsw env create <name> && lsw build");
            Ok(ExitCode::SUCCESS)
        }

        DotnetCmd::Doctor => {
            let (_p, env) = active_env(dirs)?;
            let report = lsw_core::dotnetops::doctor(&env)?;
            if format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                let mark = |c: lsw_core::dotnetops::Check| match c {
                    lsw_core::dotnetops::Check::Ok => "OK",
                    lsw_core::dotnetops::Check::NotConfigured => "NOT CONFIGURED",
                    lsw_core::dotnetops::Check::Missing => "MISSING",
                };
                println!("LSW C# Doctor  (RID {})\n", report.target);
                println!("  .NET SDK          {}", mark(report.sdk));
                println!("  Runtime ID        {}", mark(report.runtime_identifier));
                println!("  Self-contained    {}", mark(report.self_contained));
                println!("  Runtime execution {}", mark(report.runtime_execution));
                println!("  Native validation {}", mark(report.native_validation));
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(crate) fn sdk(op: &SdkCmd, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
    match op {
        SdkCmd::Import { name, from, force } => {
            println!("Importing SDK '{name}' from {}...", from.display());
            println!(
                "Note: you are responsible for the license terms of any Microsoft SDK content you import."
            );
            let report = lsw_core::sdkops::import(dirs, name, from, *force)?;
            println!(
                "Imported '{}' ({} files) to {}",
                report.name,
                report.files_copied,
                report.root.display()
            );
            Ok(ExitCode::SUCCESS)
        }

        SdkCmd::List => {
            let sdks = lsw_core::sdkops::list(dirs)?;
            if sdks.is_empty() {
                println!("No SDKs imported. Import one with: lsw sdk import <name> --from <path>");
            }
            for s in sdks {
                println!(
                    "{:<20} {:<10} {}",
                    s.name,
                    if s.usable { "usable" } else { "incomplete" },
                    s.source.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        SdkCmd::Remove { name } => {
            lsw_core::sdkops::remove(dirs, name)?;
            println!("Removed SDK '{name}'");
            Ok(ExitCode::SUCCESS)
        }
    }
}
