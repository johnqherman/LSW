use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lsw_core::Dirs;

use crate::cli::PackageTargetArg;
use crate::{active_env, cwd};

pub(crate) fn package(target: &PackageTargetArg, dirs: &Dirs) -> lsw_core::Result<ExitCode> {
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
) -> lsw_core::Result<ExitCode> {
    let (p, env) = active_env(dirs)?;
    let mapper = lsw_core::mapper(&env, &p);
    match (windows, linux) {
        (Some(path), None) => {
            let absolute = if path.is_absolute() {
                path.clone()
            } else {
                cwd().join(path)
            };
            println!("{}", mapper.to_windows(&absolute)?);
        }
        (None, Some(text)) => {
            println!("{}", mapper.to_linux(text)?.display());
        }
        _ => {
            eprintln!("usage: lsw path --windows <linux-path> | --linux <windows-path>");
            return Ok(ExitCode::FAILURE);
        }
    }
    Ok(ExitCode::SUCCESS)
}
