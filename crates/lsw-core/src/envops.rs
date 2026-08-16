use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};

use lsw_config::{
    Dirs, ENVIRONMENT_FORMAT_VERSION, EnvironmentLayout, EnvironmentManifest, LockedComponent,
    Lockfile, TargetArch, UserConfig,
};
use lsw_runtime::RuntimeProvider;
use lsw_toolchain::ProbeReport;

use crate::error::{Error, Result};
use crate::project::Project;

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Validate name.
pub fn validate_name(kind: &str, name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.ends_with(' ')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | ' '))
        || WINDOWS_RESERVED.contains(
            &name
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
        );
    if bad {
        return Err(Error::InvalidName {
            kind: kind.to_owned(),
            name: name.to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone)]
/// Environment.
pub struct Environment {
    /// Name.
    pub name: String,
    /// Layout.
    pub layout: EnvironmentLayout,
    /// Manifest.
    pub manifest: EnvironmentManifest,
}

impl Environment {
    /// Open.
    pub fn open(dirs: &Dirs, name: &str) -> Result<Self> {
        validate_name("environment", name)?;
        let root = dirs.environment(name);
        let layout = EnvironmentLayout::new(root);
        for path in [layout.root.clone(), layout.prefix(), layout.drive_c()] {
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(Error::InitFailed {
                    path,
                    detail: "managed environment path is a symlink".into(),
                });
            }
        }
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
/// Env Create Options.
pub struct EnvCreateOptions {
    /// Name.
    pub name: String,
    /// Arch.
    pub arch: TargetArch,
    /// Toolchain.
    pub toolchain: Option<String>,
    /// Sdk.
    pub sdk: Option<String>,
    /// Force.
    pub force: bool,
    /// Expose home.
    pub expose_home: bool,
}

#[derive(Debug)]
/// Env Create Report.
pub struct EnvCreateReport {
    /// Environment.
    pub environment: Environment,
    /// Probe.
    pub probe: ProbeReport,
}

/// Create.
pub fn create(dirs: &Dirs, opts: &EnvCreateOptions) -> Result<EnvCreateReport> {
    validate_name("environment", &opts.name)?;
    let root = dirs.environment(&opts.name);
    let layout = EnvironmentLayout::new(root.clone());

    if layout.manifest().is_file() && !opts.force {
        return Err(Error::EnvironmentExists {
            name: opts.name.clone(),
        });
    }

    let runtime_provider = lsw_runtime::WineRuntime;
    let resolved_runtime = runtime_provider.resolve()?;
    let (resolved_toolchain, probe) = match &opts.sdk {
        Some(sdk_name) => {
            validate_name("sdk", sdk_name)?;
            let sdk_root = dirs.sysroot(sdk_name);
            let sdk_meta = fs::symlink_metadata(&sdk_root).ok();
            if !sdk_meta.is_some_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
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

    let mut replacement = Replacement::begin(&root)?;

    for dir in dirs.managed_dirs() {
        fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
    }
    fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
    fs::create_dir_all(layout.logs()).map_err(|e| Error::io(layout.logs(), e))?;

    runtime_provider.prepare(&layout.prefix())?;

    for dir in [layout.src(), layout.temp()] {
        fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
    }
    provision_profile(&layout)?;
    if !opts.expose_home {
        harden_profiles(&layout)?;
    }

    let manifest = EnvironmentManifest {
        name: opts.name.clone(),
        format: ENVIRONMENT_FORMAT_VERSION,
        target_arch: opts.arch,
        toolchain: resolved_toolchain,
        runtime: resolved_runtime,
    };
    manifest.save(&layout.manifest())?;

    let environment = Environment {
        name: opts.name.clone(),
        layout,
        manifest,
    };
    let _ = crate::registryops::delete(
        &environment,
        "HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\RunServices",
        Some("winemenubuilder"),
    );
    if let Err(e) = crate::registryops::set(
        &environment,
        "HKLM\\Software\\Microsoft\\Windows NT\\CurrentVersion\\AeDebug",
        "Debugger",
        "false",
        "string",
    ) {
        tracing::warn!(
            error = %e,
            "could not disable AeDebug; crashing Windows processes may hang in winedbg (set [runtime] under WINEDEBUG or rerun `lsw env create --force`)"
        );
    }

    replacement.commit();
    Ok(EnvCreateReport { environment, probe })
}

#[derive(Debug)]
/// Env Summary.
pub struct EnvSummary {
    /// Name.
    pub name: String,
    /// Arch.
    pub arch: TargetArch,
    /// Toolchain.
    pub toolchain: String,
    /// Runtime.
    pub runtime: String,
    /// Healthy.
    pub healthy: bool,
}

/// List.
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
                        env.manifest.toolchain.provider,
                        env.manifest
                            .toolchain
                            .version
                            .split(" (")
                            .next()
                            .unwrap_or_default()
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

/// Remove.
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

/// Clone env.
pub fn clone_env(dirs: &Dirs, src: &str, dst: &str, force: bool) -> Result<Environment> {
    validate_name("environment", src)?;
    validate_name("environment", dst)?;
    if src == dst {
        return Err(Error::EnvironmentExists {
            name: dst.to_owned(),
        });
    }
    Environment::open(dirs, src)?;
    let src_root = dirs.environment(src);
    let dst_root = dirs.environment(dst);
    if dst_root.exists() && !force {
        return Err(Error::EnvironmentExists {
            name: dst.to_owned(),
        });
    }
    let mut replacement = Replacement::begin(&dst_root)?;
    fs::create_dir_all(&dst_root).map_err(|e| Error::io(dst_root.clone(), e))?;
    let status = std::process::Command::new("cp")
        .arg("--reflink=auto")
        .arg("-a")
        .arg(format!("{}/.", src_root.display()))
        .arg(&dst_root)
        .status()
        .map_err(|e| Error::io(PathBuf::from("cp"), e))?;
    if !status.success() {
        return Err(Error::InitFailed {
            path: dst_root,
            detail: format!("copying environment '{src}' failed"),
        });
    }
    let layout = EnvironmentLayout::new(dst_root);
    let mut manifest = EnvironmentManifest::load(&layout.manifest())?;
    dst.clone_into(&mut manifest.name);
    manifest.save(&layout.manifest())?;
    let opened = Environment::open(dirs, dst)?;
    replacement.commit();
    Ok(opened)
}

/// Restore.
pub fn restore(dirs: &Dirs, project: &Project, name: &str) -> Result<EnvCreateReport> {
    validate_name("environment", name)?;
    let lock = Lockfile::load(&project.lockfile_path())?;
    let provider = lock.toolchain.provider.clone();
    if !matches!(provider.as_str(), "llvm-mingw" | "mingw-gcc") {
        return Err(Error::RestoreUnsupportedToolchain {
            provider,
            name: name.to_owned(),
        });
    }
    let root = dirs.environment(name);
    let mut replacement = Replacement::begin(&root)?;
    let report = create(
        dirs,
        &EnvCreateOptions {
            name: name.to_owned(),
            arch: lock.target_arch,
            toolchain: Some(provider),
            sdk: None,
            force: true,
            expose_home: false,
        },
    )?;
    crate::buildops::check_lock(project, &report.environment)?;
    replacement.commit();
    Ok(report)
}

/// Use environment.
pub fn use_environment(dirs: &Dirs, project: &mut Project, name: &str) -> Result<()> {
    Environment::open(dirs, name)?;
    project.manifest.environment.name = Some(name.to_owned());
    project.save_manifest()
}

/// Resolve active.
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

fn profile_dir(layout: &EnvironmentLayout) -> PathBuf {
    layout
        .drive_c()
        .join("users")
        .join(crate::runops::windows_user())
}

/// Harden profiles.
pub fn harden_profiles(layout: &EnvironmentLayout) -> Result<usize> {
    let drive_c = layout.drive_c();
    let users = drive_c.join("users");
    let mut trimmed = 0;
    let entries = match fs::read_dir(&users) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(Error::io(users, e)),
    };
    for user in entries.take(1_000_000) {
        let user = user.map_err(|e| Error::io(users.clone(), e))?;
        let udir = user.path();
        let meta = fs::symlink_metadata(&udir).map_err(|e| Error::io(udir.clone(), e))?;
        if !meta.is_dir() {
            continue;
        }
        let inner = fs::read_dir(&udir).map_err(|e| Error::io(udir.clone(), e))?;
        for entry in inner.take(1_000_000) {
            let entry = entry.map_err(|e| Error::io(udir.clone(), e))?;
            let link = entry.path();
            let Ok(target) = fs::read_link(&link) else {
                continue;
            };
            let resolved = if target.is_absolute() {
                target
            } else {
                udir.join(target)
            };
            let resolved = lexical_normalize(&resolved);
            if !resolved.starts_with(&drive_c) {
                fs::remove_file(&link).map_err(|e| Error::io(link.clone(), e))?;
                fs::create_dir_all(&link).map_err(|e| Error::io(link.clone(), e))?;
                trimmed += 1;
            }
        }
    }
    Ok(trimmed)
}

static BACKUP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn backup_path(root: &Path) -> PathBuf {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let counter = BACKUP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    root.with_file_name(format!(
        ".{name}.lsw-bak-{}-{nanos}-{counter}",
        std::process::id()
    ))
}

struct Replacement {
    root: PathBuf,
    backup: Option<PathBuf>,
    committed: bool,
}

impl Replacement {
    fn begin(root: &Path) -> Result<Self> {
        let backup = if fs::symlink_metadata(root).is_ok() {
            let bak = backup_path(root);
            let _ = fs::remove_dir_all(&bak);
            fs::rename(root, &bak).map_err(|e| Error::io(root.to_path_buf(), e))?;
            Some(bak)
        } else {
            None
        };
        Ok(Self {
            root: root.to_path_buf(),
            backup,
            committed: false,
        })
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for Replacement {
    fn drop(&mut self) {
        if self.committed {
            if let Some(bak) = &self.backup {
                let _ = fs::remove_dir_all(bak);
            }
        } else {
            let _ = fs::remove_dir_all(&self.root);
            if let Some(bak) = &self.backup {
                let _ = fs::rename(bak, &self.root);
            }
        }
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.last(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !matches!(out.last(), Some(Component::RootDir | Component::Prefix(_))) {
                    out.push(comp);
                }
            }
            other => out.push(other),
        }
    }
    out.iter().collect()
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

/// Link project.
pub fn link_project(env: &Environment, project: &Project) -> Result<PathBuf> {
    validate_name("project", &project.manifest.project.name)?;
    let src_dir = env.layout.src();
    if let Ok(meta) = fs::symlink_metadata(&src_dir)
        && meta.file_type().is_symlink()
    {
        return Err(Error::InitFailed {
            path: src_dir,
            detail: "prefix src directory is a symlink; refusing to link through it".into(),
        });
    }
    fs::create_dir_all(&src_dir).map_err(|e| Error::io(src_dir.clone(), e))?;
    let link = src_dir.join(&project.manifest.project.name);

    if let Ok(existing) = fs::read_link(&link) {
        if existing == project.root {
            return Ok(link);
        }
        fs::remove_file(&link).map_err(|e| Error::io(link.clone(), e))?;
    } else if link.exists() {
        return Err(Error::InitFailed {
            path: link,
            detail: "exists inside the prefix but is not a symlink; remove it manually".into(),
        });
    }

    std::os::unix::fs::symlink(&project.root, &link).map_err(|e| Error::io(link.clone(), e))?;
    Ok(link)
}

/// Mapper.
pub fn mapper(env: &Environment, project: &Project) -> lsw_path::PathMapper {
    lsw_path::PathMapper::for_environment(
        &env.layout.drive_c(),
        &project.root,
        &project.manifest.project.name,
    )
}

/// Export env.
pub fn export_env(dirs: &Dirs, name: &str, file: &Path) -> Result<()> {
    validate_name("environment", name)?;
    let root = dirs.environment(name);
    if !root.is_dir() {
        return Err(Error::EnvironmentNotFound {
            name: name.to_owned(),
        });
    }
    let status = std::process::Command::new("tar")
        .arg("--zstd")
        .arg("-cf")
        .arg(file)
        .arg("-C")
        .arg(dirs.environments())
        .arg(name)
        .status()
        .map_err(|e| Error::io(std::path::PathBuf::from("tar"), e))?;
    if !status.success() {
        return Err(Error::InitFailed {
            path: file.to_path_buf(),
            detail: "tar failed to create the environment archive".into(),
        });
    }
    Ok(())
}

/// Import env.
pub fn import_env(dirs: &Dirs, name: &str, file: &Path, force: bool) -> Result<()> {
    validate_name("environment", name)?;
    let root = dirs.environment(name);
    if fs::symlink_metadata(&root).is_ok() && !force {
        return Err(Error::EnvironmentExists {
            name: name.to_owned(),
        });
    }
    let environments = dirs.environments();
    fs::create_dir_all(&environments).map_err(|e| Error::io(environments.clone(), e))?;
    let staging = backup_path(&root);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .map_err(|e| Error::io(staging.clone(), e))?;
    let staging_guard = CleanupDir(staging.clone());
    let status = std::process::Command::new("tar")
        .arg("--zstd")
        .arg("-xf")
        .arg(file)
        .arg("-C")
        .arg(&staging)
        .arg("--")
        .arg(name)
        .status()
        .map_err(|e| Error::io(std::path::PathBuf::from("tar"), e))?;
    if !status.success() {
        return Err(Error::InitFailed {
            path: file.to_path_buf(),
            detail: "tar failed to extract the environment archive".into(),
        });
    }
    let candidate = staging.join(name);
    let candidate_meta = fs::symlink_metadata(&candidate).ok();
    if !candidate_meta.is_some_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return Err(Error::InitFailed {
            path: file.to_path_buf(),
            detail: format!("archive did not contain an environment named '{name}'"),
        });
    }
    let candidate_layout = EnvironmentLayout::new(candidate.clone());
    let manifest = EnvironmentManifest::load(&candidate_layout.manifest())?;
    if manifest.name != name {
        return Err(Error::InitFailed {
            path: file.to_path_buf(),
            detail: format!("archive environment is named '{}' instead of '{name}'", manifest.name),
        });
    }
    for path in [
        candidate_layout.prefix(),
        candidate_layout.drive_c(),
        candidate_layout.src(),
        candidate_layout.temp(),
        candidate_layout.logs(),
    ] {
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::InitFailed {
                path,
                detail: "archive contains a symlink at a managed environment path".into(),
            });
        }
    }
    let mut replacement = Replacement::begin(&root)?;
    fs::rename(&candidate, &root).map_err(|e| Error::io(root.clone(), e))?;
    Environment::open(dirs, name)?;
    replacement.commit();
    drop(staging_guard);
    Ok(())
}

struct CleanupDir(PathBuf);

impl Drop for CleanupDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Provision winetricks.
pub fn provision_winetricks(
    env: &Environment,
    verbs: &[String],
) -> Result<std::process::ExitStatus> {
    let Some(winetricks) = crate::buildops::which("winetricks") else {
        return Err(Error::ToolMissing {
            tool: "winetricks".into(),
            fix: "install winetricks from your package manager".into(),
        });
    };
    let mut command = std::process::Command::new(winetricks);
    lsw_runtime::scrub_wine_env(&mut command);
    command
        .arg("-q")
        .args(verbs)
        .env("WINEPREFIX", env.layout.prefix());
    command
        .status()
        .map_err(|e| Error::io(std::path::PathBuf::from("winetricks"), e))
}

/// Lockfile for.
pub fn lockfile_for(env: &Environment, project: Option<&Project>) -> Result<Lockfile> {
    let tc = &env.manifest.toolchain;
    let rt = &env.manifest.runtime;
    let sysroot_fingerprint = fingerprint_sysroot(&tc.sysroot)?;
    let dependencies = project
        .map(|p| crate::depsops::locked_deps(p, env.manifest.target_arch))
        .unwrap_or_default();
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
            provider: tc.provider.clone(),
            version: tc.version.clone(),
            sha256: sysroot_fingerprint,
        },
        dependencies,
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
                .take(1_000_000)
                .map(|e| {
                    let meta_len = e.metadata().map_or(0, |m| m.len());
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
    fn clone_env_copies_and_renames() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: tmp.path().to_path_buf(),
            config: tmp.path().join("cfg"),
            cache: tmp.path().join("cache"),
        };
        let layout = EnvironmentLayout::new(dirs.environment("base"));
        fs::create_dir_all(layout.prefix()).unwrap();
        let manifest = EnvironmentManifest {
            name: "base".into(),
            format: ENVIRONMENT_FORMAT_VERSION,
            target_arch: TargetArch::X86_64,
            toolchain: lsw_config::ResolvedToolchain {
                provider: "llvm-mingw".into(),
                version: "1".into(),
                cc: "/cc".into(),
                cxx: "/cxx".into(),
                sysroot: "/s".into(),
                c_flags: vec![],
                cxx_flags: vec![],
                link_flags: vec![],
            },
            runtime: lsw_config::ResolvedRuntime {
                provider: "wine".into(),
                version: "9".into(),
                executable: "/wine".into(),
            },
        };
        manifest.save(&layout.manifest()).unwrap();

        let cloned = clone_env(&dirs, "base", "copy", false).unwrap();
        assert_eq!(cloned.name, "copy");
        assert_eq!(cloned.manifest.name, "copy");
        assert!(dirs.environment("copy").join("prefix").is_dir());
    }

    #[test]
    fn import_env_does_not_extract_sibling_environments() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: tmp.path().join("data"),
            config: tmp.path().join("cfg"),
            cache: tmp.path().join("cache"),
        };
        let archive_root = tmp.path().join("archive");
        let layout = EnvironmentLayout::new(archive_root.join("safe"));
        for path in [layout.src(), layout.temp(), layout.logs()] {
            fs::create_dir_all(path).unwrap();
        }
        EnvironmentManifest {
            name: "safe".into(),
            format: ENVIRONMENT_FORMAT_VERSION,
            target_arch: TargetArch::X86_64,
            toolchain: lsw_config::ResolvedToolchain {
                provider: "llvm-mingw".into(),
                version: "1".into(),
                cc: "/cc".into(),
                cxx: "/cxx".into(),
                sysroot: "/s".into(),
                c_flags: vec![],
                cxx_flags: vec![],
                link_flags: vec![],
            },
            runtime: lsw_config::ResolvedRuntime {
                provider: "wine".into(),
                version: "9".into(),
                executable: "/wine".into(),
            },
        }
        .save(&layout.manifest())
        .unwrap();
        fs::create_dir_all(archive_root.join("victim")).unwrap();
        fs::write(archive_root.join("victim/marker"), b"archive").unwrap();
        let archive = tmp.path().join("env.tar.zst");
        let status = std::process::Command::new("tar")
            .args(["--zstd", "-cf"])
            .arg(&archive)
            .arg("-C")
            .arg(&archive_root)
            .args(["safe", "victim"])
            .status()
            .unwrap();
        assert!(status.success());

        fs::create_dir_all(dirs.environment("victim")).unwrap();
        fs::write(dirs.environment("victim").join("marker"), b"original").unwrap();
        import_env(&dirs, "safe", &archive, false).unwrap();

        assert!(Environment::open(&dirs, "safe").is_ok());
        assert_eq!(
            fs::read(dirs.environment("victim").join("marker")).unwrap(),
            b"original"
        );
    }

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
    fn open_rejects_symlinked_environment_root() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs {
            data: tmp.path().join("data"),
            config: tmp.path().join("cfg"),
            cache: tmp.path().join("cache"),
        };
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(dirs.environments()).unwrap();
        std::os::unix::fs::symlink(&outside, dirs.environment("linked")).unwrap();

        assert!(Environment::open(&dirs, "linked").is_err());
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
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "../../etc",
            "x\0y",
            "$(touch owned)",
            "name;command",
            "name`command`",
        ] {
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
