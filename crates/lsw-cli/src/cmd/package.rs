use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::{Format, PackageTargetArg};
use crate::{active_env, cwd};

pub(crate) fn package(
    target: &PackageTargetArg,
    verify: bool,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    if verify && !matches!(target, PackageTargetArg::Msi) {
        return Ok(crate::usage_failure(
            format,
            "--verify requires --target msi",
        ));
    }
    let (p, env) = active_env(dirs)?;
    let target = match target {
        PackageTargetArg::PortableDirectory => {
            lsw_core::packageops::PackageTarget::PortableDirectory
        }
        PackageTargetArg::Zip => lsw_core::packageops::PackageTarget::Zip,
        PackageTargetArg::Msi => lsw_core::packageops::PackageTarget::Msi,
        PackageTargetArg::Msix => lsw_core::packageops::PackageTarget::Msix,
    };
    let report = lsw_core::packageops::package(&p, &env, target)?;
    let verified = if verify && let Some(msi) = &report.msi {
        Some(lsw_core::installops::verify_msi(
            dirs,
            &env,
            &p.manifest.project.name,
            msi,
            &report.files,
        )?)
    } else {
        None
    };
    if format == Format::Json {
        let opt_path = |p: &Option<PathBuf>| p.as_ref().map(|x| x.display().to_string());
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "package": {
                    "directory": report.directory.display().to_string(),
                    "zip": opt_path(&report.zip),
                    "msi": opt_path(&report.msi),
                    "msix": opt_path(&report.msix),
                    "files": report.files,
                },
                "verified": verified.map(|c| serde_json::json!({
                    "files": c.files.len(),
                    "install_dir": c.install_dir.display().to_string(),
                })),
            }))
            .expect("serializes")
        );
        return Ok(ExitCode::SUCCESS);
    }
    println!("Packaged: {}", report.directory.display());
    for f in &report.files {
        println!("  {f}");
    }
    if let Some(zip) = &report.zip {
        println!("Archive:  {}", zip.display());
    }
    if let Some(msi) = &report.msi {
        println!("Installer: {}", msi.display());
    }
    if let Some(msix) = &report.msix {
        println!("MSIX:      {} (self-signed)", msix.display());
    }
    if let Some(check) = &verified {
        println!(
            "Verified:  installed {} file(s) to {}, uninstalled clean",
            check.files.len(),
            check.install_dir.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn sign(file: &Path, publisher: &Option<String>) -> lsw_core::Result<ExitCode> {
    lsw_core::signops::sign(file, publisher.as_deref())?;
    println!("signed {}", file.display());
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn path(
    windows: &Option<PathBuf>,
    linux: &Option<String>,
    dirs: &Dirs,
    format: Format,
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let mapper = lsw_core::mapper(&env, &p);
    let (key, value) = match (windows, linux) {
        (Some(path), None) => {
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                cwd()?.join(path)
            };
            ("windows", mapper.to_windows(&absolute)?)
        }
        (None, Some(text)) => ("linux", mapper.to_linux(text)?.display().to_string()),
        _ => {
            return Ok(crate::usage_failure(
                format,
                "specify exactly one of --windows <linux-path> or --linux <windows-path>",
            ));
        }
    };
    if format == Format::Json {
        println!("{}", serde_json::json!({ key: value }));
    } else {
        println!("{value}");
    }
    Ok(ExitCode::SUCCESS)
}
