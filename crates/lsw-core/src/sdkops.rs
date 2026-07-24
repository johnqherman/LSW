use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use lsw_config::Dirs;

use crate::envops::validate_name;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkManifest {
    pub name: String,
    pub source: PathBuf,
    pub has_include: bool,
    pub has_lib: bool,
}

impl SdkManifest {
    fn path(root: &Path) -> PathBuf {
        root.join("sdk.toml")
    }
}

#[derive(Debug)]
pub struct SdkImportReport {
    pub name: String,
    pub root: PathBuf,
    pub files_copied: usize,
}

fn has_sdk_dir(root: &Path, kind: &str) -> bool {
    let cap = format!("{}{}", kind[..1].to_uppercase(), &kind[1..]);
    [root.to_path_buf(), root.join("crt"), root.join("sdk")]
        .iter()
        .any(|base| base.join(kind).is_dir() || base.join(&cap).is_dir())
}

pub fn import(dirs: &Dirs, name: &str, from: &Path, force: bool) -> Result<SdkImportReport> {
    validate_name("sdk", name)?;
    let from_meta = fs::symlink_metadata(from).map_err(|e| Error::io(from.to_path_buf(), e))?;
    if from_meta.file_type().is_symlink() || !from_meta.is_dir() {
        return Err(Error::SdkImportFailed {
            path: from.to_path_buf(),
            detail: "source is not a directory (or is a symlink)".into(),
        });
    }

    let root = dirs.sysroot(name);
    if root.exists() {
        if force {
            fs::remove_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;
        } else {
            return Err(Error::SdkExists {
                name: name.to_owned(),
            });
        }
    }
    fs::create_dir_all(&root).map_err(|e| Error::io(root.clone(), e))?;

    let files_copied = copy_tree(from, &root)?;

    let manifest = SdkManifest {
        name: name.to_owned(),
        source: from.to_path_buf(),
        has_include: has_sdk_dir(&root, "include"),
        has_lib: has_sdk_dir(&root, "lib"),
    };
    manifest_save(&manifest, &SdkManifest::path(&root))?;

    Ok(SdkImportReport {
        name: name.to_owned(),
        root,
        files_copied,
    })
}

#[derive(Debug)]
pub struct SdkSummary {
    pub name: String,
    pub source: PathBuf,
    pub usable: bool,
}

pub fn list(dirs: &Dirs) -> Result<Vec<SdkSummary>> {
    let root = dirs.sysroots();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&root)
        .map_err(|e| Error::io(root.clone(), e))?
        .flatten()
    {
        if !entry.path().is_dir() {
            continue;
        }
        let manifest_path = SdkManifest::path(&entry.path());
        let (source, usable) = match manifest_load(&manifest_path) {
            Ok(m) => (m.source, m.has_include && m.has_lib),
            Err(_) => (PathBuf::new(), false),
        };
        out.push(SdkSummary {
            name: entry.file_name().to_string_lossy().into_owned(),
            source,
            usable,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn remove(dirs: &Dirs, name: &str) -> Result<()> {
    validate_name("sdk", name)?;
    let root = dirs.sysroot(name);
    if !root.is_dir() {
        return Err(Error::SdkNotFound {
            name: name.to_owned(),
        });
    }
    fs::remove_dir_all(&root).map_err(|e| Error::io(root, e))
}

fn copy_tree(src: &Path, dst: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(src)
        .map_err(|e| Error::io(src.to_path_buf(), e))?
        .flatten()
    {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = match fs::symlink_metadata(&from) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            fs::create_dir_all(&to).map_err(|e| Error::io(to.clone(), e))?;
            count += copy_tree(&from, &to)?;
        } else if meta.is_file() {
            fs::copy(&from, &to).map_err(|e| Error::io(from.clone(), e))?;
            count += 1;
        }
    }
    Ok(count)
}

fn manifest_save(manifest: &SdkManifest, path: &Path) -> Result<()> {
    let text = toml::to_string_pretty(manifest).map_err(|e| Error::InitFailed {
        path: path.to_path_buf(),
        detail: format!("cannot serialize sdk manifest: {e}"),
    })?;
    fs::write(path, text).map_err(|e| Error::io(path.to_path_buf(), e))
}

fn manifest_load(path: &Path) -> Result<SdkManifest> {
    use std::io::Read;
    const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
    let file = fs::File::open(path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    let mut text = String::new();
    file.take(MAX_MANIFEST)
        .read_to_string(&mut text)
        .map_err(|e| Error::io(path.to_path_buf(), e))?;
    toml::from_str(&text).map_err(|e| Error::InitFailed {
        path: path.to_path_buf(),
        detail: format!("invalid sdk manifest: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dirs(base: &Path) -> Dirs {
        Dirs {
            data: base.join("data"),
            config: base.join("cfg"),
            cache: base.join("cache"),
        }
    }

    #[test]
    fn import_copies_tree_and_records_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());

        let src = tmp.path().join("winsdk");
        fs::create_dir_all(src.join("include/um")).unwrap();
        fs::create_dir_all(src.join("lib/x64")).unwrap();
        fs::write(src.join("include/um/windows.h"), b"// header").unwrap();
        fs::write(src.join("lib/x64/kernel32.lib"), b"lib").unwrap();

        let report = import(&dirs, "win11-sdk", &src, false).unwrap();
        assert_eq!(report.files_copied, 2);
        assert!(report.root.join("include/um/windows.h").is_file());
        assert!(report.root.join("sdk.toml").is_file());

        let listed = list(&dirs).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "win11-sdk");
        assert!(listed[0].usable);
    }

    #[test]
    fn import_refuses_duplicate_without_force() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        let src = tmp.path().join("sdk");
        fs::create_dir_all(&src).unwrap();

        import(&dirs, "x", &src, false).unwrap();
        let err = import(&dirs, "x", &src, false).unwrap_err();
        assert!(err.to_string().contains("LSW2019"));
        assert!(import(&dirs, "x", &src, true).is_ok());
    }

    #[test]
    fn hostile_names_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        let src = tmp.path().join("sdk");
        fs::create_dir_all(&src).unwrap();
        assert!(import(&dirs, "../escape", &src, false).is_err());
    }

    #[test]
    fn remove_deletes_sysroot() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = temp_dirs(tmp.path());
        let src = tmp.path().join("sdk");
        fs::create_dir_all(&src).unwrap();
        import(&dirs, "gone", &src, false).unwrap();
        remove(&dirs, "gone").unwrap();
        assert!(list(&dirs).unwrap().is_empty());
        assert!(
            remove(&dirs, "gone")
                .unwrap_err()
                .to_string()
                .contains("LSW2020")
        );
    }
}
