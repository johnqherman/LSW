use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

const MAX_LISTING_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DEP_DEPTH: usize = 64;
const MAX_DEP_NODES: usize = 100_000;
const MAX_DIR_ENTRIES: usize = 1_000_000;

const SYSTEM_DLLS: &[&str] = &[
    "kernel32.dll",
    "kernelbase.dll",
    "ntdll.dll",
    "user32.dll",
    "gdi32.dll",
    "advapi32.dll",
    "shell32.dll",
    "shlwapi.dll",
    "ole32.dll",
    "oleaut32.dll",
    "combase.dll",
    "comctl32.dll",
    "comdlg32.dll",
    "ws2_32.dll",
    "wininet.dll",
    "winhttp.dll",
    "crypt32.dll",
    "bcrypt.dll",
    "msvcrt.dll",
    "ucrtbase.dll",
    "rpcrt4.dll",
    "sechost.dll",
    "setupapi.dll",
    "version.dll",
    "winmm.dll",
];

fn is_system_dll(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("api-ms-win-")
        || lower.starts_with("ext-ms-win-")
        || SYSTEM_DLLS.contains(&lower.as_str())
}

struct DllIndex {
    by_name: Vec<std::collections::HashMap<String, PathBuf>>,
}

impl DllIndex {
    fn build(dirs: &[PathBuf]) -> Self {
        let by_name = dirs
            .iter()
            .map(|dir| {
                let mut map = std::collections::HashMap::new();
                let Ok(entries) = std::fs::read_dir(dir) else {
                    return map;
                };
                for entry in entries.flatten().take(MAX_DIR_ENTRIES) {
                    let lower = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    if entry.path().is_file() {
                        map.entry(lower).or_insert_with(|| entry.path());
                    }
                }
                map
            })
            .collect();
        Self { by_name }
    }

    fn resolve(&self, name: &str) -> Option<PathBuf> {
        let wanted = name.to_ascii_lowercase();
        self.by_name
            .iter()
            .find_map(|map| map.get(&wanted).cloned())
    }
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Dep Kind.
pub enum DepKind {
    /// Root.
    Root,
    /// System.
    System,
    /// Resolved.
    Resolved,
    /// Missing.
    Missing,
    /// Seen.
    Seen,
}

#[derive(Debug, Serialize)]
/// Dep Node.
pub struct DepNode {
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: DepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Path.
    pub path: Option<String>,
    /// Children.
    pub children: Vec<DepNode>,
}

fn search_dirs(env: Option<&Environment>, pe: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(parent) = pe.parent() {
        dirs.push(parent.to_path_buf());
    }
    if let Some(env) = env {
        dirs.push(env.manifest.toolchain.sysroot.join("bin"));
        dirs.push(env.layout.drive_c().join("windows/system32"));
    }
    dirs
}

fn build(
    name: &str,
    path: &Path,
    dirs: &DllIndex,
    seen: &mut BTreeSet<String>,
    depth: usize,
    nodes: &mut usize,
) -> Vec<DepNode> {
    let mut children = Vec::new();
    if depth >= MAX_DEP_DEPTH {
        return children;
    }
    let Ok(imports) = lsw_pe::imports(path) else {
        return children;
    };
    let mut names: Vec<String> = imports;
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names.dedup_by_key(|n| n.to_ascii_lowercase());
    for dep in names {
        if dep.eq_ignore_ascii_case(name) {
            continue;
        }
        if *nodes >= MAX_DEP_NODES {
            break;
        }
        children.push(node(&dep, dirs, seen, depth + 1, nodes));
    }
    children
}

fn node(
    name: &str,
    dirs: &DllIndex,
    seen: &mut BTreeSet<String>,
    depth: usize,
    nodes: &mut usize,
) -> DepNode {
    *nodes += 1;
    let key = name.to_ascii_lowercase();
    if is_system_dll(name) || *nodes >= MAX_DEP_NODES {
        return DepNode {
            name: name.to_owned(),
            kind: DepKind::System,
            path: None,
            children: Vec::new(),
        };
    }
    match dirs.resolve(name) {
        Some(resolved) => {
            if !seen.insert(key) {
                return DepNode {
                    name: name.to_owned(),
                    kind: DepKind::Seen,
                    path: Some(resolved.display().to_string()),
                    children: Vec::new(),
                };
            }
            let children = build(name, &resolved, dirs, seen, depth, nodes);
            DepNode {
                name: name.to_owned(),
                kind: DepKind::Resolved,
                path: Some(resolved.display().to_string()),
                children,
            }
        }
        None => DepNode {
            name: name.to_owned(),
            kind: DepKind::Missing,
            path: None,
            children: Vec::new(),
        },
    }
}

/// Tree.
pub fn tree(env: Option<&Environment>, pe: &Path) -> Result<DepNode> {
    tree_with_dirs(&search_dirs(env, pe), pe)
}

/// Tree with dirs.
pub fn tree_with_dirs(dirs: &[PathBuf], pe: &Path) -> Result<DepNode> {
    if !pe.is_file() {
        return Err(Error::NotExecutable {
            program: pe.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    lsw_pe::detect(pe)?;
    let index = DllIndex::build(dirs);
    let mut seen = BTreeSet::new();
    let mut nodes = 0usize;
    let name = pe
        .file_name()
        .map_or_else(|| "root".to_owned(), |n| n.to_string_lossy().into_owned());
    let children = build(&name, pe, &index, &mut seen, 0, &mut nodes);
    Ok(DepNode {
        name,
        kind: DepKind::Root,
        path: Some(pe.display().to_string()),
        children,
    })
}

pub(crate) fn is_vc_runtime_dll(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    std::path::Path::new(&lower)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        && (lower.starts_with("vcruntime")
            || lower.starts_with("msvcp")
            || lower.starts_with("concrt"))
}

pub(crate) fn vc_runtime_dirs(sysroot: &Path) -> Vec<PathBuf> {
    const MAX_SCAN_DEPTH: usize = 6;
    const MAX_SCAN_ENTRIES: usize = 50_000;
    let mut out = BTreeSet::new();
    let mut budget = MAX_SCAN_ENTRIES;
    scan_vc_dirs(sysroot, MAX_SCAN_DEPTH, &mut budget, &mut out);
    out.into_iter().collect()
}

fn scan_vc_dirs(dir: &Path, depth: usize, budget: &mut usize, out: &mut BTreeSet<PathBuf>) {
    if depth == 0 || *budget == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Ok(ftype) = entry.file_type() else {
            continue;
        };
        if ftype.is_symlink() {
            continue;
        }
        if ftype.is_dir() {
            scan_vc_dirs(&entry.path(), depth - 1, budget, out);
        } else if ftype.is_file() && is_vc_runtime_dll(&entry.file_name().to_string_lossy()) {
            out.insert(dir.to_path_buf());
        }
    }
}

const MIRROR: &str = "https://repo.msys2.org/mingw";

#[derive(Debug, Clone, Serialize)]
/// Pkg Ref.
pub struct PkgRef {
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Filename.
    pub filename: String,
    /// Sha256.
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
/// Installed Dep.
pub struct InstalledDep {
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
}

pub(crate) fn repo_for(arch: lsw_config::TargetArch) -> Result<(&'static str, &'static str)> {
    use lsw_config::TargetArch::{X86_64, X86, Aarch64};
    match arch {
        X86_64 => Ok(("mingw64", "mingw-w64-x86_64")),
        X86 => Ok(("mingw32", "mingw-w64-i686")),
        Aarch64 => Ok(("clangarm64", "mingw-w64-clang-aarch64")),
        other => Err(Error::DepArchUnsupported {
            arch: format!("{other:?}").to_lowercase(),
        }),
    }
}

fn deps_root(project: &crate::project::Project, arch: lsw_config::TargetArch) -> PathBuf {
    let arch = format!("{arch:?}").to_lowercase();
    project.root.join("deps").join(arch)
}

pub(crate) fn locked_deps(
    project: &crate::project::Project,
    arch: lsw_config::TargetArch,
) -> std::collections::BTreeMap<String, lsw_config::LockedDep> {
    let meta = deps_root(project, arch).join(".lsw");
    project
        .manifest
        .dependencies
        .iter()
        .map(|(name, version)| {
            let sha256 =
                crate::buildops::read_capped_string(&meta.join(format!("{name}.sha256")), 1024)
                    .map(|s| s.trim().to_owned())
                    .unwrap_or_default();
            (
                name.clone(),
                lsw_config::LockedDep {
                    version: version.clone(),
                    sha256,
                },
            )
        })
        .collect()
}

fn curl_download(url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
    }
    let out = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "2",
            "--max-filesize",
            "1073741824",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .output()
        .map_err(|e| Error::io(PathBuf::from("curl"), e))?;
    if !out.status.success() {
        return Err(Error::DownloadFailed {
            url: url.to_owned(),
            detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(())
}

fn refresh_db(dirs: &lsw_config::Dirs, repo: &str) -> Result<PathBuf> {
    let cache = dirs.cache.join("msys2").join(repo);
    let db = cache.join(format!("{repo}.db"));
    curl_download(&format!("{MIRROR}/{repo}/{repo}.db"), &db)?;
    let extracted = cache.join("db");
    let _ = std::fs::remove_dir_all(&extracted);
    std::fs::create_dir_all(&extracted).map_err(|e| Error::io(extracted.clone(), e))?;
    let out = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&db)
        .arg("-C")
        .arg(&extracted)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| Error::io(PathBuf::from("tar"), e))?;
    if !out.success() {
        return Err(Error::ExtractFailed {
            name: format!("{repo}.db"),
            detail: "extracting repository database failed".to_owned(),
        });
    }
    Ok(extracted)
}

fn dep_root_contained(project: &crate::project::Project, root: &Path) -> bool {
    let is_symlink =
        |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    if is_symlink(&project.root.join("deps")) || is_symlink(root) {
        return false;
    }
    match (root.canonicalize(), project.root.canonicalize()) {
        (Ok(r), Ok(p)) => r.starts_with(&p),
        _ => false,
    }
}

fn desc_field(desc: &str, key: &str) -> Option<String> {
    let mut lines = desc.lines();
    while let Some(line) = lines.next() {
        if line.trim() == key {
            return lines.next().map(|v| v.trim().to_owned());
        }
    }
    None
}

fn resolve(dirs: &lsw_config::Dirs, repo: &str, prefix: &str, name: &str) -> Result<PkgRef> {
    let full = format!("{prefix}-{name}");
    let extracted = refresh_db(dirs, repo)?;
    for entry in std::fs::read_dir(&extracted)
        .map_err(|e| Error::io(extracted.clone(), e))?
        .flatten()
        .take(MAX_DIR_ENTRIES)
    {
        let desc_path = entry.path().join("desc");
        let Some(desc) = crate::buildops::read_capped_string(&desc_path, 4 * 1024 * 1024) else {
            continue;
        };
        if desc_field(&desc, "%NAME%").as_deref() == Some(full.as_str()) {
            return Ok(PkgRef {
                name: name.to_owned(),
                version: desc_field(&desc, "%VERSION%").unwrap_or_default(),
                filename: desc_field(&desc, "%FILENAME%").unwrap_or_default(),
                sha256: desc_field(&desc, "%SHA256SUM%").unwrap_or_default(),
            });
        }
    }
    Err(Error::DepNotFound {
        name: name.to_owned(),
        repo: repo.to_owned(),
    })
}

fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !Path::new(name).is_absolute()
}

/// Add.
pub fn add(
    project: &crate::project::Project,
    arch: lsw_config::TargetArch,
    dirs: &lsw_config::Dirs,
    name: &str,
) -> Result<PkgRef> {
    use std::io::Read;
    let (repo, prefix) = repo_for(arch)?;
    let pkg = resolve(dirs, repo, prefix, name)?;
    if !is_safe_filename(&pkg.filename) {
        return Err(Error::DepNotFound {
            name: name.to_owned(),
            repo: repo.to_owned(),
        });
    }
    let cached = dirs.cache.join("msys2").join(repo).join(&pkg.filename);
    if !cached.is_file() {
        curl_download(&format!("{MIRROR}/{repo}/{}", pkg.filename), &cached)?;
    }
    if !pkg.sha256.is_empty() {
        let actual = crate::sha256_file_checked(&cached)?;
        if !actual.eq_ignore_ascii_case(&pkg.sha256) {
            let _ = std::fs::remove_file(&cached);
            return Err(Error::ChecksumMismatch {
                name: name.to_owned(),
                expected: pkg.sha256,
                actual,
            });
        }
    }

    let root = deps_root(project, arch);
    std::fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
    if !dep_root_contained(project, &root) {
        return Err(Error::ExtractFailed {
            name: name.to_owned(),
            detail: "dependency directory resolves outside the project; refusing to extract"
                .to_owned(),
        });
    }
    let mut listing_child = std::process::Command::new("tar")
        .arg("--zstd")
        .arg("-tf")
        .arg(&cached)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| Error::io(PathBuf::from("tar"), e))?;
    let mut stdout = listing_child.stdout.take().expect("piped stdout");
    let mut captured = Vec::new();
    let read_result = (&mut stdout)
        .take(MAX_LISTING_BYTES + 1)
        .read_to_end(&mut captured);
    let _ = std::io::copy(&mut stdout, &mut std::io::sink());
    let listing_status = listing_child
        .wait()
        .map_err(|e| Error::io(PathBuf::from("tar"), e))?;
    read_result.map_err(|e| Error::io(PathBuf::from("tar"), e))?;
    let too_big = captured.len() as u64 > MAX_LISTING_BYTES;
    if too_big {
        return Err(Error::ExtractFailed {
            name: name.to_owned(),
            detail: format!("archive listing exceeds {MAX_LISTING_BYTES}-byte limit"),
        });
    }
    if !listing_status.success() {
        return Err(Error::ExtractFailed {
            name: name.to_owned(),
            detail: "listing archive contents failed".to_owned(),
        });
    }
    let files: Vec<String> = String::from_utf8_lossy(&captured)
        .lines()
        .filter_map(|l| l.trim().split_once('/').map(|(_, rest)| rest.to_owned()))
        .filter(|p| !p.is_empty() && !p.ends_with('/'))
        .collect();

    let extract = std::process::Command::new("tar")
        .arg("--zstd")
        .arg("-xf")
        .arg(&cached)
        .arg("--strip-components=1")
        .arg("--exclude=.*")
        .arg("-C")
        .arg(&root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| Error::io(PathBuf::from("tar"), e))?;
    if !extract.success() {
        return Err(Error::ExtractFailed {
            name: name.to_owned(),
            detail: "extracting package archive failed".to_owned(),
        });
    }

    let meta_dir = root.join(".lsw");
    if std::fs::symlink_metadata(&meta_dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::ExtractFailed {
            name: name.to_owned(),
            detail: "dependency metadata directory is a symlink".to_owned(),
        });
    }
    std::fs::create_dir_all(&meta_dir).map_err(|e| Error::io(meta_dir.clone(), e))?;
    let files_path = meta_dir.join(format!("{name}.files"));
    if std::fs::symlink_metadata(&files_path).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(&files_path).map_err(|e| Error::io(files_path.clone(), e))?;
    }
    std::fs::write(&files_path, files.join("\n")).map_err(|e| Error::io(files_path.clone(), e))?;
    let sha_path = meta_dir.join(format!("{name}.sha256"));
    if std::fs::symlink_metadata(&sha_path).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(&sha_path).map_err(|e| Error::io(sha_path.clone(), e))?;
    }
    std::fs::write(&sha_path, &pkg.sha256).map_err(|e| Error::io(sha_path.clone(), e))?;

    let manifest_path = project.root.join("lsw.toml");
    let mut manifest = lsw_config::ProjectManifest::load(&manifest_path)?;
    manifest
        .dependencies
        .insert(name.to_owned(), pkg.version.clone());
    manifest.save(&manifest_path)?;
    Ok(pkg)
}

/// Vendor.
pub fn vendor(
    project: &crate::project::Project,
    arch: lsw_config::TargetArch,
    src: &Path,
) -> Result<usize> {
    let meta = std::fs::symlink_metadata(src).map_err(|e| Error::io(src.to_path_buf(), e))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::ExtractFailed {
            name: src.display().to_string(),
            detail: "vendor source is not a directory (or is a symlink)".into(),
        });
    }
    let has_any = ["include", "lib", "bin"]
        .iter()
        .any(|d| src.join(d).is_dir());
    if !has_any {
        return Err(Error::ExtractFailed {
            name: src.display().to_string(),
            detail: "vendor source has no include/, lib/, or bin/ subdirectory".into(),
        });
    }
    let root = deps_root(project, arch);
    std::fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
    if !dep_root_contained(project, &root) {
        return Err(Error::ExtractFailed {
            name: src.display().to_string(),
            detail: "dependency directory resolves outside the project; refusing to copy".into(),
        });
    }
    let mut copied = 0usize;
    for sub in ["include", "lib", "bin"] {
        let from = src.join(sub);
        if from.is_dir() {
            copied += copy_dir_capped(&from, &root.join(sub), 64, &mut 0)?;
        }
    }
    let name = src.file_name().map_or_else(
        || "vendored".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let name = crate::project::sanitize_project_name(&name);
    let manifest_path = project.root.join("lsw.toml");
    let mut manifest = lsw_config::ProjectManifest::load(&manifest_path)?;
    manifest.dependencies.insert(name, "vendored".to_owned());
    manifest.save(&manifest_path)?;
    Ok(copied)
}

fn copy_dir_capped(src: &Path, dst: &Path, depth: usize, visited: &mut usize) -> Result<usize> {
    const MAX_ENTRIES: usize = 500_000;
    if depth == 0 {
        return Err(Error::ExtractFailed {
            name: src.display().to_string(),
            detail: "vendor tree is too deep".into(),
        });
    }
    std::fs::create_dir_all(dst).map_err(|e| Error::io(dst.to_path_buf(), e))?;
    let mut copied = 0;
    for entry in std::fs::read_dir(src)
        .map_err(|e| Error::io(src.to_path_buf(), e))?
        .flatten()
    {
        *visited += 1;
        if *visited > MAX_ENTRIES {
            return Err(Error::ExtractFailed {
                name: src.display().to_string(),
                detail: format!("vendor tree exceeds {MAX_ENTRIES} entries"),
            });
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let to = dst.join(entry.file_name());
        if meta.is_dir() {
            copied += copy_dir_capped(&entry.path(), &to, depth - 1, visited)?;
        } else if meta.is_file() {
            std::fs::copy(entry.path(), &to).map_err(|e| Error::io(entry.path(), e))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// Remove.
pub fn remove(
    project: &crate::project::Project,
    arch: lsw_config::TargetArch,
    name: &str,
) -> Result<bool> {
    let manifest_path = project.root.join("lsw.toml");
    let mut manifest = lsw_config::ProjectManifest::load(&manifest_path)?;
    if manifest.dependencies.remove(name).is_none() {
        return Ok(false);
    }

    let root = deps_root(project, arch);
    if name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !dep_root_contained(project, &root)
    {
        manifest.save(&manifest_path)?;
        return Ok(true);
    }
    let files_manifest = root.join(".lsw").join(format!("{name}.files"));
    if let (Some(list), Ok(canon_root)) = (
        crate::buildops::read_capped_string(&files_manifest, 16 * 1024 * 1024),
        root.canonicalize(),
    ) {
        for rel in list.lines() {
            let rel = rel.trim();
            if rel.is_empty() {
                continue;
            }
            let relp = std::path::Path::new(rel);
            if relp.is_absolute()
                || relp
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                continue;
            }
            let target = root.join(relp);
            let within = target
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .is_some_and(|p| p.starts_with(&canon_root));
            if !within {
                continue;
            }
            if let Err(e) = std::fs::remove_file(&target)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return Err(Error::io(target, e));
            }
        }
    }
    let _ = std::fs::remove_file(&files_manifest);
    manifest.save(&manifest_path)?;
    Ok(true)
}

/// List.
pub fn list(project: &crate::project::Project) -> Vec<InstalledDep> {
    project
        .manifest
        .dependencies
        .iter()
        .map(|(name, version)| InstalledDep {
            name: name.clone(),
            version: version.clone(),
        })
        .collect()
}

/// Dep dirs.
pub fn dep_dirs(
    project: &crate::project::Project,
    arch: lsw_config::TargetArch,
) -> Option<(PathBuf, PathBuf, PathBuf)> {
    if project.manifest.dependencies.is_empty() {
        return None;
    }
    let root = deps_root(project, arch);
    let include = root.join("include");
    let lib = root.join("lib");
    let bin = root.join("bin");
    if include.is_dir() || lib.is_dir() {
        Some((include, lib, bin))
    } else {
        None
    }
}

/// Returns the vcpkg triplet for the given target architecture.
pub fn vcpkg_triplet(arch: lsw_config::TargetArch) -> Result<&'static str> {
    use lsw_config::TargetArch::{Aarch64, X86, X86_64};
    match arch {
        X86_64 => Ok("x64-mingw-static"),
        X86 => Ok("x86-mingw-static"),
        Aarch64 => Ok("arm64-mingw-static"),
        other => Err(Error::DepArchUnsupported {
            arch: format!("{other:?}").to_lowercase(),
        }),
    }
}

/// Returns the managed vcpkg root directory.
pub fn vcpkg_root(dirs: &lsw_config::Dirs) -> PathBuf {
    dirs.data.join("vcpkg")
}

/// Bootstraps vcpkg by cloning the repository if absent and running the bootstrap script.
pub fn vcpkg_bootstrap(dirs: &lsw_config::Dirs) -> Result<PathBuf> {
    let root = vcpkg_root(dirs);
    let exe = root.join("vcpkg");
    if exe.is_file() {
        return Ok(exe);
    }
    if !root.join(".git").is_dir() {
        std::fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
        let out = std::process::Command::new("git")
            .args(["clone", "--depth", "1", "https://github.com/microsoft/vcpkg.git"])
            .arg(&root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| Error::io(PathBuf::from("git"), e))?;
        if !out.status.success() {
            let _ = std::fs::remove_dir_all(&root);
            return Err(Error::DownloadFailed {
                url: "https://github.com/microsoft/vcpkg.git".into(),
                detail: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
    }
    let bootstrap = root.join("bootstrap-vcpkg.sh");
    let out = std::process::Command::new("sh")
        .arg(&bootstrap)
        .current_dir(&root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| Error::io(bootstrap.clone(), e))?;
    if !out.status.success() {
        return Err(Error::ToolMissing {
            tool: "vcpkg".into(),
            fix: format!(
                "bootstrap failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        });
    }
    Ok(exe)
}

#[derive(Debug, Serialize)]
/// Report from a vcpkg install operation.
pub struct VcpkgReport {
    /// Packages that were installed.
    pub installed: Vec<String>,
    /// The vcpkg triplet used.
    pub triplet: String,
}

/// Installs one or more vcpkg packages for the environment's target architecture.
pub fn vcpkg_install(
    dirs: &lsw_config::Dirs,
    env: &Environment,
    packages: &[String],
) -> Result<VcpkgReport> {
    let triplet = vcpkg_triplet(env.manifest.target_arch)?;
    let vcpkg = vcpkg_bootstrap(dirs)?;
    let root = vcpkg_root(dirs);
    let mut installed = Vec::new();
    for pkg in packages {
        let spec = format!("{pkg}:{triplet}");
        let out = std::process::Command::new(&vcpkg)
            .args(["install", &spec])
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| Error::io(vcpkg.clone(), e))?;
        if !out.status.success() {
            return Err(Error::DepNotFound {
                name: pkg.clone(),
                repo: format!("vcpkg ({triplet})"),
            });
        }
        installed.push(pkg.clone());
    }
    Ok(VcpkgReport {
        installed,
        triplet: triplet.to_owned(),
    })
}

/// Returns vcpkg installed include/lib paths for the given architecture, if any packages are installed.
pub fn vcpkg_dirs(dirs: &lsw_config::Dirs, arch: lsw_config::TargetArch) -> Option<(PathBuf, PathBuf)> {
    let Ok(triplet) = vcpkg_triplet(arch) else {
        return None;
    };
    let root = vcpkg_root(dirs);
    let installed = root.join("installed").join(triplet);
    let include = installed.join("include");
    let lib = installed.join("lib");
    if include.is_dir() || lib.is_dir() {
        Some((include, lib))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desc_field_reads_named_sections() {
        let desc = "%NAME%\nmingw-w64-x86_64-zlib\n\n%VERSION%\n1.3.1-1\n";
        assert_eq!(
            desc_field(desc, "%NAME%").as_deref(),
            Some("mingw-w64-x86_64-zlib")
        );
        assert_eq!(desc_field(desc, "%VERSION%").as_deref(), Some("1.3.1-1"));
        assert_eq!(desc_field(desc, "%MISSING%"), None);
    }

    #[test]
    fn is_system_dll_matches_apisets_and_known_modules() {
        assert!(is_system_dll("KERNEL32.dll"));
        assert!(is_system_dll("api-ms-win-crt-runtime-l1-1-0.dll"));
        assert!(!is_system_dll("libstdc++-6.dll"));
    }

    #[test]
    fn resolve_dll_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("libFoo-1.dll"), b"x").unwrap();
        let index = DllIndex::build(&[dir.path().to_path_buf()]);
        assert!(index.resolve("libfoo-1.dll").is_some());
        assert!(index.resolve("missing.dll").is_none());
    }

    #[test]
    fn vcpkg_triplet_maps_arches() {
        assert_eq!(
            vcpkg_triplet(lsw_config::TargetArch::X86_64).unwrap(),
            "x64-mingw-static"
        );
        assert_eq!(
            vcpkg_triplet(lsw_config::TargetArch::X86).unwrap(),
            "x86-mingw-static"
        );
        assert_eq!(
            vcpkg_triplet(lsw_config::TargetArch::Aarch64).unwrap(),
            "arm64-mingw-static"
        );
        assert!(vcpkg_triplet(lsw_config::TargetArch::Armv7).is_err());
    }

    #[test]
    fn vcpkg_root_is_under_data_dir() {
        let dirs = lsw_config::Dirs {
            config: PathBuf::from("/home/u/.config/lsw"),
            data: PathBuf::from("/home/u/.local/share/lsw"),
            cache: PathBuf::from("/home/u/.cache/lsw"),
        };
        assert_eq!(
            vcpkg_root(&dirs),
            PathBuf::from("/home/u/.local/share/lsw/vcpkg")
        );
    }

    #[test]
    fn vcpkg_dirs_returns_none_when_not_installed() {
        let dirs = lsw_config::Dirs {
            config: PathBuf::from("/nonexistent/config"),
            data: PathBuf::from("/nonexistent/data"),
            cache: PathBuf::from("/nonexistent/cache"),
        };
        assert!(vcpkg_dirs(&dirs, lsw_config::TargetArch::X86_64).is_none());
    }
}
