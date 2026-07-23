use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::{Format, PackageTargetArg};
use crate::{active_env, cwd};

pub(crate) fn package(
    target: &PackageTargetArg,
    verify: bool,
    dirs: &Dirs,
) -> lsw_core::Result<ExitCode> {
    if verify && !matches!(target, PackageTargetArg::Msi) {
        eprintln!("--verify requires --target msi");
        return Ok(ExitCode::FAILURE);
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
    if verify && let Some(msi) = &report.msi {
        let check = lsw_core::installops::verify_msi(
            dirs,
            &env,
            &p.manifest.project.name,
            msi,
            &report.files,
        )?;
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
            eprintln!("usage: lsw path --windows <linux-path> | --linux <windows-path>");
            return Ok(ExitCode::FAILURE);
        }
    };
    if format == Format::Json {
        println!("{}", serde_json::json!({ key: value }));
    } else {
        println!("{value}");
    }
    Ok(ExitCode::SUCCESS)
}
