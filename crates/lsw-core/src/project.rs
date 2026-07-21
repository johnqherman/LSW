use std::fs;
use std::path::{Path, PathBuf};

use lsw_config::{PROJECT_MANIFEST, ProjectManifest};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
}

impl Project {
    pub fn discover(start: &Path) -> Result<Self> {
        let (root, manifest) = ProjectManifest::discover(start)?;
        if manifest.target.os != "windows" {
            return Err(Error::UnsupportedTargetOs {
                os: manifest.target.os,
            });
        }
        Ok(Self { root, manifest })
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(PROJECT_MANIFEST)
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.root.join(lsw_config::PROJECT_LOCKFILE)
    }

    pub fn save_manifest(&self) -> Result<()> {
        Ok(self.manifest.save(&self.manifest_path())?)
    }
}

const TEMPLATE_MAIN: &str = r#"#include <windows.h>
#include <stdio.h>

// Console hello. For a GUI variant, switch WIN32_EXECUTABLE on in
// CMakeLists.txt and use WinMain + MessageBoxA instead.
int main(void) {
    printf("Hello from LSW\n");
    printf("Running on tick %lu\n", (unsigned long)GetTickCount());
    return 0;
}
"#;

const TEMPLATE_CMAKE: &str = r#"cmake_minimum_required(VERSION 3.20)
project({name} C)

add_executable({name} src/main.c)

# For a GUI application (no console window):
# set_target_properties({name} PROPERTIES WIN32_EXECUTABLE ON)
"#;

const TEMPLATE_GITIGNORE: &str = "build/\n";

#[derive(Debug)]
pub struct InitReport {
    pub root: PathBuf,
    pub created: Vec<PathBuf>,
}

pub fn init(parent: &Path, name: Option<&str>) -> Result<InitReport> {
    let (root, project_name) = match name {
        Some(n) => (parent.join(n), n.to_owned()),
        None => {
            let n = parent
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or_else(|| Error::InitFailed {
                    path: parent.to_path_buf(),
                    detail: "cannot derive a project name from this directory".into(),
                })?;
            (parent.to_path_buf(), n)
        }
    };

    let manifest_path = root.join(PROJECT_MANIFEST);
    if manifest_path.exists() {
        return Err(Error::InitFailed {
            path: root,
            detail: "lsw.toml already exists here".into(),
        });
    }

    fn write_file(path: &PathBuf, contents: &str, created: &mut Vec<PathBuf>) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io(dir.to_path_buf(), e))?;
        }
        fs::write(path, contents).map_err(|e| Error::io(path.clone(), e))?;
        created.push(path.clone());
        Ok(())
    }

    let mut created = Vec::new();
    ProjectManifest::new(&project_name).save(&manifest_path)?;
    created.push(manifest_path);

    write_file(
        &root.join("CMakeLists.txt"),
        &TEMPLATE_CMAKE.replace("{name}", &project_name),
        &mut created,
    )?;
    write_file(&root.join("src/main.c"), TEMPLATE_MAIN, &mut created)?;
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        write_file(&gitignore, TEMPLATE_GITIGNORE, &mut created)?;
    }

    Ok(InitReport { root, created })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_named_creates_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let report = init(dir.path(), Some("hello")).unwrap();
        assert_eq!(report.root, dir.path().join("hello"));
        assert!(report.root.join("lsw.toml").is_file());
        assert!(report.root.join("CMakeLists.txt").is_file());
        assert!(report.root.join("src/main.c").is_file());

        let project = Project::discover(&report.root).unwrap();
        assert_eq!(project.manifest.project.name, "hello");
    }

    #[test]
    fn init_in_place_uses_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("myapp");
        fs::create_dir(&root).unwrap();
        let report = init(&root, None).unwrap();
        let project = Project::discover(&report.root).unwrap();
        assert_eq!(project.manifest.project.name, "myapp");
    }

    #[test]
    fn init_refuses_double_init() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), Some("x")).unwrap();
        let err = init(dir.path(), Some("x")).unwrap_err();
        assert!(err.to_string().contains("LSW2009"));
    }

    #[test]
    fn discover_rejects_non_windows_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = ProjectManifest::new("x");
        m.target.os = "linux".into();
        m.save(&dir.path().join(PROJECT_MANIFEST)).unwrap();
        let err = Project::discover(dir.path()).unwrap_err();
        assert!(err.to_string().contains("LSW2008"));
    }
}
