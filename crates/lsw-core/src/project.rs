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
        crate::envops::validate_name("project", &manifest.project.name)?;
        if manifest.target.os != "windows" {
            return Err(Error::UnsupportedTargetOs {
                os: manifest.target.os,
            });
        }
        if manifest.filesystem.project_drive != "C:" || manifest.filesystem.mount_project != "/src"
        {
            return Err(Error::UnsupportedFilesystem {
                drive: manifest.filesystem.project_drive.clone(),
                mount: manifest.filesystem.mount_project.clone(),
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Template {
    #[default]
    Console,
    Gui,
    Dll,
}

const CONSOLE_MAIN: &str = r#"#include <windows.h>
#include <stdio.h>

int main(void) {
    printf("Hello from LSW\n");
    printf("Running on tick %lu\n", (unsigned long)GetTickCount());
    return 0;
}
"#;

const GUI_MAIN: &str = r#"#include <windows.h>

int WINAPI WinMain(HINSTANCE inst, HINSTANCE prev, LPSTR cmd, int show) {
    (void)inst; (void)prev; (void)cmd; (void)show;
    MessageBoxA(NULL, "Hello from LSW", "LSW", MB_OK | MB_ICONINFORMATION);
    return 0;
}
"#;

const DLL_MAIN: &str = r#"#include <windows.h>

__declspec(dllexport) int lsw_answer(void) {
    return 42;
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved) {
    (void)inst; (void)reason; (void)reserved;
    return TRUE;
}
"#;

const CONSOLE_CMAKE: &str = r#"cmake_minimum_required(VERSION 3.20)
project({name} C)

add_executable({name} src/main.c)

enable_testing()
add_test(NAME {name}_runs COMMAND {name})
"#;

const GUI_CMAKE: &str = r#"cmake_minimum_required(VERSION 3.20)
project({name} C)

add_executable({name} WIN32 src/main.c)
"#;

const DLL_CMAKE: &str = r#"cmake_minimum_required(VERSION 3.20)
project({name} C)

add_library({name} SHARED src/main.c)
set_target_properties({name} PROPERTIES PREFIX "")
"#;

fn template_sources(template: Template) -> (&'static str, &'static str) {
    match template {
        Template::Console => (CONSOLE_CMAKE, CONSOLE_MAIN),
        Template::Gui => (GUI_CMAKE, GUI_MAIN),
        Template::Dll => (DLL_CMAKE, DLL_MAIN),
    }
}

const TEMPLATE_GITIGNORE: &str = "build/\n";

#[derive(Debug)]
pub struct InitReport {
    pub root: PathBuf,
    pub created: Vec<PathBuf>,
    pub existing_build: Option<String>,
}

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn sanitize_project_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        return "project".to_owned();
    }
    if WINDOWS_RESERVED_NAMES.contains(&trimmed.to_ascii_lowercase().as_str()) {
        return format!("{trimmed}-app");
    }
    trimmed.to_owned()
}

pub fn init(parent: &Path, name: Option<&str>, template: Template) -> Result<InitReport> {
    let named = name.is_some();
    let (root, project_name) = match name {
        Some(n) => {
            let sanitized = sanitize_project_name(n);
            (parent.join(&sanitized), sanitized)
        }
        None => {
            let n = parent
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or_else(|| Error::InitFailed {
                    path: parent.to_path_buf(),
                    detail: "cannot derive a project name from this directory".into(),
                })?;
            (parent.to_path_buf(), sanitize_project_name(&n))
        }
    };

    if named
        && let Ok(mut entries) = fs::read_dir(&root)
        && entries.next().is_some()
    {
        return Err(Error::InitFailed {
            path: root.clone(),
            detail: format!(
                "target directory '{}' already exists and is not empty (a distinct name may have sanitized to the same directory); pick another name or `cd` in and run `lsw init`",
                root.display()
            ),
        });
    }

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

    let existing_build = crate::buildops::detect_build_system(&root).map(|s| format!("{s:?}"));
    if existing_build.is_none() {
        let (cmake, main_c) = template_sources(template);
        write_file(
            &root.join("CMakeLists.txt"),
            &cmake.replace("{name}", &project_name),
            &mut created,
        )?;
        write_file(&root.join("src/main.c"), main_c, &mut created)?;
    }
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        write_file(&gitignore, TEMPLATE_GITIGNORE, &mut created)?;
    }

    Ok(InitReport {
        root,
        created,
        existing_build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_named_creates_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let report = init(dir.path(), Some("hello"), Template::Console).unwrap();
        assert_eq!(report.root, dir.path().join("hello"));
        assert!(report.root.join("lsw.toml").is_file());
        assert!(report.root.join("CMakeLists.txt").is_file());
        assert!(report.root.join("src/main.c").is_file());

        let project = Project::discover(&report.root).unwrap();
        assert_eq!(project.manifest.project.name, "hello");
    }

    #[test]
    fn init_templates_pick_the_right_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), Some("g"), Template::Gui).unwrap();
        let g = fs::read_to_string(dir.path().join("g/CMakeLists.txt")).unwrap();
        assert!(g.contains("WIN32"));
        assert!(
            fs::read_to_string(dir.path().join("g/src/main.c"))
                .unwrap()
                .contains("WinMain")
        );

        init(dir.path(), Some("d"), Template::Dll).unwrap();
        let d = fs::read_to_string(dir.path().join("d/CMakeLists.txt")).unwrap();
        assert!(d.contains("add_library") && d.contains("SHARED"));
        assert!(
            fs::read_to_string(dir.path().join("d/src/main.c"))
                .unwrap()
                .contains("dllexport")
        );
    }

    #[test]
    fn init_in_place_uses_directory_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("myapp");
        fs::create_dir(&root).unwrap();
        let report = init(&root, None, Template::Console).unwrap();
        let project = Project::discover(&report.root).unwrap();
        assert_eq!(project.manifest.project.name, "myapp");
    }

    #[test]
    fn init_refuses_double_init() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path(), Some("x"), Template::Console).unwrap();
        let err = init(dir.path(), Some("x"), Template::Console).unwrap_err();
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
