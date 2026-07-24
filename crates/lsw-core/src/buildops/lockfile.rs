use std::fs;

use lsw_config::Lockfile;

use crate::envops::{self, Environment};
use crate::error::{Error, Result};
use crate::project::Project;

pub(crate) fn stamp_build_dir(project: &Project, env: &Environment) -> Result<()> {
    let build_dir = project.root.join("build");
    if fs::symlink_metadata(&build_dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::InitFailed {
            path: build_dir.clone(),
            detail: "build/ is a symlink; refusing to build through it".into(),
        });
    }
    let marker = build_dir.join(".lsw-env");
    let owner = super::read_capped(&marker, 1024 * 1024).and_then(|b| String::from_utf8(b).ok());
    if build_dir.is_dir()
        && let Some(owner) = &owner
        && owner.trim() != env.name
    {
        fs::remove_dir_all(&build_dir).map_err(|e| Error::io(build_dir.clone(), e))?;
    }
    fs::create_dir_all(&build_dir).map_err(|e| Error::io(build_dir.clone(), e))?;
    super::safe_marker_write(&marker, &env.name);
    Ok(())
}

pub(crate) fn check_lock(project: &Project, env: &Environment) -> Result<()> {
    let path = project.lockfile_path();
    if !path.is_file() {
        return Ok(());
    }
    let recorded = Lockfile::load(&path)?;
    let current = envops::lockfile_for(env)?;
    if recorded != current {
        return Err(Error::LockMismatch {
            environment: env.name.clone(),
            detail: lock_diff(&recorded, &current),
        });
    }
    Ok(())
}

pub(crate) fn sync_lockfile(project: &Project, env: &Environment, update: bool) -> Result<bool> {
    let current = envops::lockfile_for(env)?;
    let path = project.lockfile_path();
    if !path.is_file() || update {
        current.save(&path)?;
        return Ok(true);
    }
    let recorded = Lockfile::load(&path)?;
    if recorded != current {
        return Err(Error::LockMismatch {
            environment: env.name.clone(),
            detail: lock_diff(&recorded, &current),
        });
    }
    Ok(false)
}

fn lock_diff(recorded: &Lockfile, current: &Lockfile) -> String {
    let mut lines = Vec::new();
    let pairs = [
        ("toolchain", &recorded.toolchain, &current.toolchain),
        ("runtime", &recorded.runtime, &current.runtime),
        ("sysroot", &recorded.sysroot, &current.sysroot),
    ];
    for (label, rec, cur) in pairs {
        if rec != cur {
            lines.push(format!(
                "  {label}: locked {} {} ({}...) but environment has {} {} ({}...)",
                rec.provider,
                rec.version,
                rec.sha256.chars().take(12).collect::<String>(),
                cur.provider,
                cur.version,
                cur.sha256.chars().take(12).collect::<String>(),
            ));
        }
    }
    if recorded.target_arch != current.target_arch {
        lines.push(format!(
            "  arch: locked {} but environment targets {}",
            recorded.target_arch, current.target_arch
        ));
    }
    lines.join("\n")
}
