use std::fs;

use lsw_config::Lockfile;

use crate::envops::{self, Environment};
use crate::error::{Error, Result};
use crate::project::Project;

pub(crate) fn stamp_build_dir(project: &Project, env: &Environment) -> Result<()> {
    let build_dir = project.root.join("build");
    let marker = build_dir.join(".lsw-env");
    if build_dir.is_dir() {
        let owner = fs::read_to_string(&marker).unwrap_or_default();
        if owner.trim() != env.name {
            fs::remove_dir_all(&build_dir).map_err(|e| Error::io(build_dir.clone(), e))?;
        }
    }
    fs::create_dir_all(&build_dir).map_err(|e| Error::io(build_dir.clone(), e))?;
    fs::write(&marker, &env.name).map_err(|e| Error::io(marker, e))
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
                &rec.sha256[..12.min(rec.sha256.len())],
                cur.provider,
                cur.version,
                &cur.sha256[..12.min(cur.sha256.len())],
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
