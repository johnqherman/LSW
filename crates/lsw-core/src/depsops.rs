use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::envops::Environment;
use crate::error::{Error, Result};

const MAX_LISTING_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DEP_DEPTH: usize = 64;

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

fn resolve_dll(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let wanted = name.to_ascii_lowercase();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().to_ascii_lowercase() == wanted
                && entry.path().is_file()
            {
                return Some(entry.path());
            }
        }
    }
    None
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    Root,
    System,
    Resolved,
    Missing,
    Seen,
}

#[derive(Debug, Serialize)]
pub struct DepNode {
    pub name: String,
    pub kind: DepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
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
    dirs: &[PathBuf],
    seen: &mut BTreeSet<String>,
    depth: usize,
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
        children.push(node(&dep, dirs, seen, depth + 1));
    }
    children
}

fn node(name: &str, dirs: &[PathBuf], seen: &mut BTreeSet<String>, depth: usize) -> DepNode {
    let key = name.to_ascii_lowercase();
    if is_system_dll(name) {
        return DepNode {
            name: name.to_owned(),
            kind: DepKind::System,
            path: None,
            children: Vec::new(),
        };
    }
    match resolve_dll(name, dirs) {
        Some(resolved) => {
            if !seen.insert(key) {
                return DepNode {
                    name: name.to_owned(),
                    kind: DepKind::Seen,
                    path: Some(resolved.display().to_string()),
                    children: Vec::new(),
                };
            }
            let children = build(name, &resolved, dirs, seen, depth);
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

pub fn tree(env: Option<&Environment>, pe: &Path) -> Result<DepNode> {
    if !pe.is_file() {
        return Err(Error::NotExecutable {
            program: pe.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    lsw_pe::detect(pe)?;
    let dirs = search_dirs(env, pe);
    let mut seen = BTreeSet::new();
    let name = pe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_owned());
    let children = build(&name, pe, &dirs, &mut seen, 0);
    Ok(DepNode {
        name,
        kind: DepKind::Root,
        path: Some(pe.display().to_string()),
        children,
    })
}

const MIRROR: &str = "https://repo.msys2.org/mingw";

#[derive(Debug, Clone, Serialize)]
pub struct PkgRef {
    pub name: String,
    pub version: String,
    pub filename: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledDep {
    pub name: String,
    pub version: String,
}

fn repo_for(arch: lsw_config::TargetArch) -> Result<(&'static str, &'static str)> {
    use lsw_config::TargetArch::*;
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

fn read_capped_string(path: &Path, max: u64) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(max).read_to_end(&mut buf).ok()?;
    String::from_utf8(buf).ok()
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
    {
        let desc_path = entry.path().join("desc");
        let Some(desc) = read_capped_string(&desc_path, 4 * 1024 * 1024) else {
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

fn sha256_of(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| Error::io(path.to_path_buf(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !Path::new(name).is_absolute()
}

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
        let actual = sha256_of(&cached)?;
        if !actual.eq_ignore_ascii_case(&pkg.sha256) {
            let _ = std::fs::remove_file(&cached);
            return Err(Error::ChecksumMismatch {
                name: name.to_owned(),
                expected: pkg.sha256.clone(),
                actual,
            });
        }
    }

    let root = deps_root(project, arch);
    std::fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
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
    std::fs::create_dir_all(&meta_dir).map_err(|e| Error::io(meta_dir.clone(), e))?;
    std::fs::write(meta_dir.join(format!("{name}.files")), files.join("\n"))
        .map_err(|e| Error::io(meta_dir.clone(), e))?;

    let manifest_path = project.root.join("lsw.toml");
    let mut manifest = lsw_config::ProjectManifest::load(&manifest_path)?;
    manifest
        .dependencies
        .insert(name.to_owned(), pkg.version.clone());
    manifest.save(&manifest_path)?;
    Ok(pkg)
}

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
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        manifest.save(&manifest_path)?;
        return Ok(true);
    }
    let files_manifest = root.join(".lsw").join(format!("{name}.files"));
    if let (Some(list), Ok(canon_root)) = (
        read_capped_string(&files_manifest, 16 * 1024 * 1024),
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
        let dirs = vec![dir.path().to_path_buf()];
        assert!(resolve_dll("libfoo-1.dll", &dirs).is_some());
        assert!(resolve_dll("missing.dll", &dirs).is_none());
    }
}
