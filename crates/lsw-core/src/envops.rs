use std::fs;
use std::path::{Path, PathBuf};

use lsw_config::{
    Dirs, ENVIRONMENT_FORMAT_VERSION, EnvironmentLayout, EnvironmentManifest, LockedComponent,
    Lockfile, TargetArch, UserConfig,
};
use lsw_runtime::RuntimeProvider;
use lsw_toolchain::ProbeReport;

use crate::error::{Error, Result};
use crate::project::Project;

pub fn validate_name(kind: &str, name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0');
    if bad {
        return Err(Error::InvalidName {
            kind: kind.to_owned(),
            name: name.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub name: String,
    pub layout: EnvironmentLayout,
    pub manifest: EnvironmentManifest,
}

impl Environment {
    pub fn open(dirs: &Dirs, name: &str) -> Result<Self> {
        validate_name("environment", name)?;
        let root = dirs.environment(name);
        let layout = EnvironmentLayout::new(root);
        if !layout.manifest().is_file() {
            return Err(Error::EnvironmentNotFound {
                name: name.to_owned(),
            });
        }
        let manifest = EnvironmentManifest::load(&layout.manifest())?;
        Ok(Self {
            name: name.to_owned(),
            layout,
            manifest,
        })
    }
}

#[derive(Debug)]
pub struct EnvCreateOptions {
    pub name: String,
    pub arch: TargetArch,
    pub toolchain: Option<String>,
    pub sdk: Option<String>,
    pub force: bool,
    pub expose_home: bool,
}

#[derive(Debug)]
pub struct EnvCreateReport {
    pub environment: Environment,
    pub probe: ProbeReport,
}

pub fn create(dirs: &Dirs, opts: &EnvCreateOptions) -> Result<EnvCreateReport> {
    validate_name("environment", &opts.name)?;
    let root = dirs.environment(&opts.name);
    let layout = EnvironmentLayout::new(root.clone());

    if layout.manifest().is_file() {
        if opts.force {
            fs::remove_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
        } else {
            return Err(Error::EnvironmentExists {
                name: opts.name.clone(),
            });
        }
    }

    fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
    fs::create_dir_all(layout.logs()).map_err(|e| Error::io(layout.logs(), e))?;

    let runtime_provider = lsw_runtime::WineRuntime;
    let resolved_runtime = runtime_provider.resolve()?;
    runtime_provider.prepare(&layout.prefix())?;

    for dir in [layout.src(), layout.temp()] {
        fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
    }
    provision_profile(&layout)?;
    if !opts.expose_home {
        harden_profiles(&layout)?;
    }

    let (resolved_toolchain, probe) = match &opts.sdk {
        Some(sdk_name) => {
            validate_name("sdk", sdk_name)?;
            let sdk_root = dirs.sysroot(sdk_name);
            if !sdk_root.is_dir() {
                return Err(Error::SdkNotFound {
                    name: sdk_name.clone(),
                });
            }
            let tc = lsw_toolchain::resolve_msvc(opts.arch, &sdk_root)?;
            let probe = lsw_toolchain::probe_msvc(&tc);
            (tc, probe)
        }
        None => lsw_toolchain::select(opts.toolchain.as_deref(), opts.arch)?,
    };

    let manifest = EnvironmentManifest {
        name: opts.name.clone(),
        format: ENVIRONMENT_FORMAT_VERSION,
        target_arch: opts.arch,
        toolchain: resolved_toolchain,
        runtime: resolved_runtime,
    };
    manifest.save(&layout.manifest())?;

    Ok(EnvCreateReport {
        environment: Environment {
            name: opts.name.clone(),
            layout,
            manifest,
        },
        probe,
    })
}

#[derive(Debug)]
pub struct EnvSummary {
    pub name: String,
    pub arch: TargetArch,
    pub toolchain: String,
    pub runtime: String,
    pub healthy: bool,
}

pub fn list(dirs: &Dirs) -> Result<Vec<EnvSummary>> {
    let root = dirs.environments();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let entries = fs::read_dir(&root).map_err(|e| Error::io(root.clone(), e))?;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        match Environment::open(dirs, &name) {
            Ok(env) => {
                let diag = lsw_runtime::WineRuntime.diagnostics(&env.layout.prefix());
                out.push(EnvSummary {
                    name,
                    arch: env.manifest.target_arch,
                    toolchain: format!(
                        "{} {}",
                        env.manifest.toolchain.provider, env.manifest.toolchain.version
                    ),
                    runtime: format!(
                        "{} {}",
                        env.manifest.runtime.provider, env.manifest.runtime.version
                    ),
                    healthy: diag.prefix_initialized,
                });
            }
            Err(_) => out.push(EnvSummary {
                name,
                arch: TargetArch::X86_64,
                toolchain: "<unreadable>".into(),
                runtime: "<unreadable>".into(),
                healthy: false,
            }),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn remove(dirs: &Dirs, name: &str) -> Result<()> {
    validate_name("environment", name)?;
    let root = dirs.environment(name);
    if !root.is_dir() {
        return Err(Error::EnvironmentNotFound {
            name: name.to_owned(),
        });
    }
    fs::remove_dir_all(&root).map_err(|e| Error::io(root, e))
}

pub fn use_environment(dirs: &Dirs, project: &mut Project, name: &str) -> Result<()> {
    Environment::open(dirs, name)?;
    project.manifest.environment.name = Some(name.to_owned());
    project.save_manifest()
}

pub fn resolve_active(dirs: &Dirs, project: &Project) -> Result<Environment> {
    let from_manifest = project.manifest.environment.name.clone();
    let name = match from_manifest {
        Some(n) => n,
        None => UserConfig::load_default()?
            .default_environment
            .ok_or(Error::NoActiveEnvironment)?,
    };
    Environment::open(dirs, &name)
}

pub fn profile_dir(layout: &EnvironmentLayout) -> PathBuf {
    layout
        .drive_c()
        .join("users")
        .join(crate::runops::WINDOWS_USER)
}

pub fn harden_profiles(layout: &EnvironmentLayout) -> Result<usize> {
    let drive_c = layout.drive_c();
    let users = drive_c.join("users");
    let mut trimmed = 0;
    let entries = match fs::read_dir(&users) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };
    for user in entries.flatten() {
        let udir = user.path();
        if !udir.is_dir() {
            continue;
        }
        let inner = match fs::read_dir(&udir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in inner.flatten() {
            let link = entry.path();
            let Ok(target) = fs::read_link(&link) else {
                continue;
            };
            let resolved = if target.is_absolute() {
                target
            } else {
                udir.join(target)
            };
            if !resolved.starts_with(&drive_c) {
                fs::remove_file(&link).map_err(|e| Error::io(link.clone(), e))?;
                fs::create_dir_all(&link).map_err(|e| Error::io(link.clone(), e))?;
                trimmed += 1;
            }
        }
    }
    Ok(trimmed)
}

fn provision_profile(layout: &EnvironmentLayout) -> Result<()> {
    let profile = profile_dir(layout);
    for sub in [
        "Desktop",
        "Documents",
        "AppData/Roaming",
        "AppData/Local",
        "AppData/LocalLow",
    ] {
        let dir = profile.join(sub);
        fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
    }
    Ok(())
}

pub fn link_project(env: &Environment, project: &Project) -> Result<PathBuf> {
    validate_name("project", &project.manifest.project.name)?;
    let src_dir = env.layout.src();
    fs::create_dir_all(&src_dir).map_err(|e| Error::io(src_dir.clone(), e))?;
    let link = src_dir.join(&project.manifest.project.name);

    if let Ok(existing) = fs::read_link(&link) {
        if existing == project.root {
            return Ok(link);
        }
        fs::remove_file(&link).map_err(|e| Error::io(link.clone(), e))?;
    } else if link.exists() {
        return Err(Error::InitFailed {
            path: link.clone(),
            detail: "exists inside the prefix but is not a symlink; remove it manually".into(),
        });
    }

    std::os::unix::fs::symlink(&project.root, &link).map_err(|e| Error::io(link.clone(), e))?;
    Ok(link)
}

pub fn mapper(env: &Environment, project: &Project) -> lsw_path::PathMapper {
    lsw_path::PathMapper::for_environment(
        &env.layout.drive_c(),
        &project.root,
        &project.manifest.project.name,
    )
}

pub fn lockfile_for(env: &Environment) -> Result<Lockfile> {
    let tc = &env.manifest.toolchain;
    let rt = &env.manifest.runtime;
    let sysroot_fingerprint = fingerprint_sysroot(&tc.sysroot)?;
    Ok(Lockfile {
        version: 1,
        environment_format: env.manifest.format,
        target_arch: env.manifest.target_arch,
        toolchain: LockedComponent {
            provider: tc.provider.clone(),
            version: tc.version.clone(),
            sha256: lsw_toolchain::sha256_file(&tc.cc).map_err(|e| Error::io(tc.cc.clone(), e))?,
        },
        runtime: LockedComponent {
            provider: rt.provider.clone(),
            version: rt.version.clone(),
            sha256: lsw_toolchain::sha256_file(&rt.executable)
                .map_err(|e| Error::io(rt.executable.clone(), e))?,
        },
        sysroot: LockedComponent {
            provider: "mingw-w64".into(),
            version: "host".into(),
            sha256: sysroot_fingerprint,
        },
    })
}

fn fingerprint_sysroot(sysroot: &Path) -> Result<String> {
    use std::fmt::Write as _;
    let include = sysroot.join("include");
    if !include.is_dir() {
        return Err(Error::io(
            include,
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "sysroot has no include directory; reinstall the mingw-w64 sysroot",
            ),
        ));
    }
    let mut summary = format!("sysroot:{}\n", sysroot.display());
    for sub in ["include", "lib"] {
        let dir = sysroot.join(sub);
        let mut names: Vec<String> = match fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .map(|e| {
                    let meta_len = e.metadata().map(|m| m.len()).unwrap_or(0);
                    format!("{}:{}", e.file_name().to_string_lossy(), meta_len)
                })
                .collect(),
            Err(_) => vec![format!("{sub}:missing")],
        };
        names.sort();
        for n in names {
            let _ = writeln!(summary, "{n}");
        }
    }
    Ok(lsw_toolchain::sha256_bytes(summary.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harden_profiles_replaces_host_escaping_symlinks_only() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = EnvironmentLayout::new(tmp.path().join("env"));
        let bob = layout.drive_c().join("users").join("bob");
        fs::create_dir_all(&bob).unwrap();
        let outside = tmp.path().join("host_home");
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, bob.join("Documents")).unwrap();
        std::os::unix::fs::symlink("AppData", bob.join("SelfLink")).unwrap();
        fs::create_dir_all(bob.join("Real")).unwrap();

        let trimmed = harden_profiles(&layout).unwrap();
        assert_eq!(trimmed, 1);
        assert!(bob.join("Documents").is_dir());
        assert!(fs::read_link(bob.join("Documents")).is_err());
        assert!(fs::read_link(bob.join("SelfLink")).is_ok());
        assert!(bob.join("Real").is_dir());
    }

    #[test]
    fn open_missing_environment_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: tmp.path().to_path_buf(),
            config: tmp.path().join("cfg"),
            cache: tmp.path().join("cache"),
        };
        let err = Environment::open(&dirs, "nope").unwrap_err();
        assert!(err.to_string().contains("LSW2002"));
    }

    #[test]
    fn list_empty_when_no_environments() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: tmp.path().to_path_buf(),
            config: tmp.path().join("cfg"),
            cache: tmp.path().join("cache"),
        };
        assert!(list(&dirs).unwrap().is_empty());
    }

    #[test]
    fn hostile_names_are_rejected_before_any_filesystem_touch() {
        for bad in ["", ".", "..", "a/b", "a\\b", "../../etc", "x\0y"] {
            let err = validate_name("environment", bad).unwrap_err();
            assert!(err.to_string().contains("LSW2012"), "accepted {bad:?}");
        }
        assert!(validate_name("environment", "win11-x64").is_ok());
        assert!(validate_name("project", "hello_app.2").is_ok());
    }

    #[test]
    fn remove_refuses_traversal_names_and_leaves_siblings_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: tmp.path().join("data"),
            config: tmp.path().join("cfg"),
            cache: tmp.path().join("cache"),
        };
        let precious = dirs.data.join("precious.txt");
        fs::create_dir_all(dirs.environments()).unwrap();
        fs::write(&precious, b"keep me").unwrap();

        for bad in ["", "..", "../..", "sub/dir"] {
            let err = remove(&dirs, bad).unwrap_err();
            assert!(err.to_string().contains("LSW2012"), "removed with {bad:?}");
        }
        assert!(precious.is_file());
        assert!(dirs.environments().is_dir());
    }
}
