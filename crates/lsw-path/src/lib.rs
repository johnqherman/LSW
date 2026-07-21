use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error(
        "LSW1201: path '{}' is not absolute; path translation requires absolute Linux paths - canonicalize it first (e.g. std::fs::canonicalize) or join it onto the project root",
        path.display()
    )]
    NotAbsolute { path: PathBuf },
    #[error(
        "LSW1202: no mapping covers '{path}'; the path must live under a mapped root (the project root or the environment's drive_c) - move it under one of those or register an additional Mapping for its root"
    )]
    Unmapped { path: String },
    #[error(
        "LSW1203: '{path}' is not a drive-letter Windows path; expected a form like C:\\dir\\file (forward slashes and a lowercase drive letter are accepted) - check that the path came from to_windows or a Windows-side tool"
    )]
    InvalidWindowsPath { path: String },
    #[error(
        "LSW1204: path '{}' contains a non-UTF-8 component that cannot be represented as a Windows path; rename the offending file or directory to valid UTF-8",
        path.display()
    )]
    NonUtf8 { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub linux: PathBuf,
    pub windows: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathMapper {
    mappings: Vec<Mapping>,
}

impl PathMapper {
    pub fn new(mappings: Vec<Mapping>) -> Self {
        let mut mappings: Vec<Mapping> = mappings
            .into_iter()
            .map(|mut m| {
                if let Ok((drive, comps)) = parse_windows(&m.windows) {
                    m.windows = render_windows(drive, &comps);
                }
                m
            })
            .collect();
        mappings.sort_by(|a, b| {
            b.linux
                .components()
                .count()
                .cmp(&a.linux.components().count())
                .then(b.linux.as_os_str().len().cmp(&a.linux.as_os_str().len()))
        });
        Self { mappings }
    }

    pub fn for_environment(drive_c: &Path, project_root: &Path, project_name: &str) -> Self {
        Self::new(vec![
            Mapping {
                linux: project_root.to_path_buf(),
                windows: format!("C:\\src\\{project_name}"),
            },
            Mapping {
                linux: drive_c.to_path_buf(),
                windows: "C:\\".to_owned(),
            },
        ])
    }

    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    pub fn to_windows(&self, path: &Path) -> Result<String, PathError> {
        if !path.is_absolute() {
            return Err(PathError::NotAbsolute {
                path: path.to_path_buf(),
            });
        }
        let path = normalize_lexically(path);
        for mapping in &self.mappings {
            let Ok(rest) = path.strip_prefix(&mapping.linux) else {
                continue;
            };
            let (drive, prefix) = parse_windows(&mapping.windows)?;
            let mut comps: Vec<String> = prefix.iter().map(|c| (*c).to_owned()).collect();
            for component in rest.components() {
                match component {
                    Component::Normal(part) => {
                        let part = part.to_str().ok_or_else(|| PathError::NonUtf8 {
                            path: path.to_path_buf(),
                        })?;
                        comps.push(part.to_owned());
                    }
                    Component::CurDir => {}
                    Component::ParentDir => {
                        return Err(PathError::Unmapped {
                            path: path.to_string_lossy().into_owned(),
                        });
                    }
                    Component::RootDir | Component::Prefix(_) => {}
                }
            }
            return Ok(render_windows(drive, &comps));
        }
        Err(PathError::Unmapped {
            path: path.to_string_lossy().into_owned(),
        })
    }

    pub fn to_linux(&self, path: &str) -> Result<PathBuf, PathError> {
        let (drive, comps) = parse_windows(path)?;
        let mut best: Option<(usize, &Mapping)> = None;
        for mapping in &self.mappings {
            let Ok((map_drive, map_comps)) = parse_windows(&mapping.windows) else {
                continue;
            };
            if map_drive == drive
                && comps.len() >= map_comps.len()
                && comps[..map_comps.len()] == map_comps[..]
                && best.is_none_or(|(depth, _)| map_comps.len() > depth)
            {
                best = Some((map_comps.len(), mapping));
            }
        }
        let (depth, mapping) = best.ok_or_else(|| PathError::Unmapped {
            path: path.to_owned(),
        })?;
        let mut out = mapping.linux.clone();
        for part in &comps[depth..] {
            out.push(part);
        }
        Ok(out)
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => out.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                let popped = out.pop();
                if !popped || out.as_os_str().is_empty() {
                    out = PathBuf::from("/");
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

fn parse_windows(path: &str) -> Result<(char, Vec<&str>), PathError> {
    let invalid = || PathError::InvalidWindowsPath {
        path: path.to_owned(),
    };
    let [drive, b':', rest @ ..] = path.as_bytes() else {
        return Err(invalid());
    };
    if !drive.is_ascii_alphabetic() || !(rest.is_empty() || rest[0] == b'\\' || rest[0] == b'/') {
        return Err(invalid());
    }
    let drive = (*drive as char).to_ascii_uppercase();
    let comps: Vec<&str> = path[2..]
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect();
    Ok((drive, comps))
}

fn render_windows(drive: char, comps: &[impl AsRef<str>]) -> String {
    let mut out = format!("{drive}:\\");
    let mut first = true;
    for comp in comps {
        if !first {
            out.push('\\');
        }
        out.push_str(comp.as_ref());
        first = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_mapper() -> (PathMapper, PathBuf, PathBuf) {
        let drive_c = PathBuf::from("/data/lsw/environments/e1/prefix/drive_c");
        let project = PathBuf::from("/home/alice/code/demo");
        let mapper = PathMapper::for_environment(&drive_c, &project, "demo");
        (mapper, drive_c, project)
    }

    #[test]
    fn project_paths_use_project_rule_even_when_project_lives_elsewhere() {
        let (mapper, _, project) = env_mapper();
        assert_eq!(
            mapper.to_windows(&project.join("src/main.c")).unwrap(),
            "C:\\src\\demo\\src\\main.c"
        );
    }

    #[test]
    fn project_rule_beats_drive_c_rule_when_project_is_inside_drive_c() {
        let drive_c = PathBuf::from("/env/prefix/drive_c");
        let project = drive_c.join("users/alice/demo");
        let mapper = PathMapper::for_environment(&drive_c, &project, "demo");
        assert_eq!(
            mapper.to_windows(&project.join("a.c")).unwrap(),
            "C:\\src\\demo\\a.c"
        );
    }

    #[test]
    fn drive_c_paths_map_to_drive_root() {
        let (mapper, drive_c, _) = env_mapper();
        assert_eq!(
            mapper.to_windows(&drive_c.join("Temp/out.log")).unwrap(),
            "C:\\Temp\\out.log"
        );
        assert_eq!(
            mapper
                .to_windows(&drive_c.join("windows/system32"))
                .unwrap(),
            "C:\\windows\\system32"
        );
    }

    #[test]
    fn bare_drive_root_keeps_trailing_backslash() {
        let (mapper, drive_c, _) = env_mapper();
        assert_eq!(mapper.to_windows(&drive_c).unwrap(), "C:\\");
    }

    #[test]
    fn mapped_directory_itself_has_no_trailing_backslash() {
        let (mapper, _, project) = env_mapper();
        assert_eq!(mapper.to_windows(&project).unwrap(), "C:\\src\\demo");
    }

    #[test]
    fn relative_path_errors_not_absolute() {
        let (mapper, _, _) = env_mapper();
        let err = mapper.to_windows(Path::new("src/main.c")).unwrap_err();
        assert!(matches!(err, PathError::NotAbsolute { .. }));
        let msg = err.to_string();
        assert!(msg.starts_with("LSW1201"), "{msg}");
        assert!(msg.contains("canonicalize"), "{msg}");
    }

    #[test]
    fn unmapped_absolute_path_errors() {
        let (mapper, _, _) = env_mapper();
        let err = mapper.to_windows(Path::new("/etc/passwd")).unwrap_err();
        assert!(matches!(err, PathError::Unmapped { .. }));
        assert!(err.to_string().starts_with("LSW1202"));
    }

    #[test]
    fn prefix_match_respects_component_boundaries() {
        let mapper = PathMapper::new(vec![Mapping {
            linux: PathBuf::from("/home/alice/code/f"),
            windows: "C:\\f".to_owned(),
        }]);
        let err = mapper
            .to_windows(Path::new("/home/alice/code/foo"))
            .unwrap_err();
        assert!(matches!(err, PathError::Unmapped { .. }));
        assert_eq!(
            mapper
                .to_windows(Path::new("/home/alice/code/f/x.c"))
                .unwrap(),
            "C:\\f\\x.c"
        );
    }

    #[test]
    fn sibling_of_project_root_does_not_match_project_rule() {
        let (mapper, _, _) = env_mapper();
        let err = mapper
            .to_windows(Path::new("/home/alice/code/demo-old/a.c"))
            .unwrap_err();
        assert!(matches!(err, PathError::Unmapped { .. }));
    }

    #[test]
    fn constructor_sorts_most_specific_first() {
        let mapper = PathMapper::new(vec![
            Mapping {
                linux: PathBuf::from("/env/drive_c"),
                windows: "C:\\".to_owned(),
            },
            Mapping {
                linux: PathBuf::from("/env/drive_c/src/demo"),
                windows: "C:\\src\\demo".to_owned(),
            },
        ]);
        assert_eq!(
            mapper.mappings()[0].linux,
            Path::new("/env/drive_c/src/demo")
        );
        assert_eq!(
            mapper
                .to_windows(Path::new("/env/drive_c/src/demo/a.c"))
                .unwrap(),
            "C:\\src\\demo\\a.c"
        );
    }

    #[test]
    fn curdir_components_are_dropped() {
        let (mapper, _, project) = env_mapper();
        let dotted = project.join("./src/./main.c");
        assert_eq!(
            mapper.to_windows(&dotted).unwrap(),
            "C:\\src\\demo\\src\\main.c"
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_linux_path_errors() {
        use std::os::unix::ffi::OsStrExt;
        let (mapper, _, project) = env_mapper();
        let bad = project.join(std::ffi::OsStr::from_bytes(b"caf\xe9.c"));
        let err = mapper.to_windows(&bad).unwrap_err();
        assert!(matches!(err, PathError::NonUtf8 { .. }));
        assert!(err.to_string().starts_with("LSW1204"));
    }

    #[test]
    fn backslash_and_forward_slash_forms_parse_identically() {
        let (mapper, _, project) = env_mapper();
        let want = project.join("src/main.c");
        assert_eq!(mapper.to_linux("C:\\src\\demo\\src\\main.c").unwrap(), want);
        assert_eq!(mapper.to_linux("c:/src/demo/src/main.c").unwrap(), want);
        assert_eq!(mapper.to_linux("c:\\src\\demo/src/main.c").unwrap(), want);
    }

    #[test]
    fn drive_letter_is_case_insensitive() {
        let (mapper, drive_c, _) = env_mapper();
        assert_eq!(
            mapper.to_linux("c:\\Temp\\a.log").unwrap(),
            drive_c.join("Temp/a.log")
        );
    }

    #[test]
    fn trailing_separator_is_tolerated() {
        let (mapper, drive_c, project) = env_mapper();
        assert_eq!(mapper.to_linux("C:\\src\\demo\\").unwrap(), project);
        assert_eq!(mapper.to_linux("C:\\").unwrap(), drive_c);
        assert_eq!(mapper.to_linux("C:/").unwrap(), drive_c);
        assert_eq!(mapper.to_linux("C:").unwrap(), drive_c);
    }

    #[test]
    fn longest_windows_prefix_wins_on_to_linux() {
        let (mapper, drive_c, project) = env_mapper();
        assert_eq!(
            mapper.to_linux("C:\\src\\demo\\a.c").unwrap(),
            project.join("a.c")
        );
        assert_eq!(
            mapper.to_linux("C:\\src\\other\\a.c").unwrap(),
            drive_c.join("src/other/a.c")
        );
    }

    #[test]
    fn unknown_drive_letter_errors_unmapped() {
        let (mapper, _, _) = env_mapper();
        let err = mapper.to_linux("D:\\foo\\bar").unwrap_err();
        assert!(matches!(err, PathError::Unmapped { .. }));
        assert!(err.to_string().starts_with("LSW1202"));
    }

    #[test]
    fn non_drive_forms_error_invalid() {
        let (mapper, _, _) = env_mapper();
        let bad_forms = [
            "src\\demo",
            "\\\\server\\share\\x",
            "",
            "C",
            "1:\\x",
            "C:relative\\x",
        ];
        for bad in bad_forms {
            let err = mapper.to_linux(bad).unwrap_err();
            assert!(
                matches!(err, PathError::InvalidWindowsPath { .. }),
                "expected InvalidWindowsPath for {bad:?}, got {err}"
            );
            assert!(err.to_string().starts_with("LSW1203"));
        }
    }

    #[test]
    fn doubled_separators_are_tolerated() {
        let (mapper, _, project) = env_mapper();
        assert_eq!(
            mapper.to_linux("C:\\src\\\\demo//src\\main.c").unwrap(),
            project.join("src/main.c")
        );
    }

    #[test]
    fn round_trip_is_identity_for_mapped_paths() {
        let (mapper, drive_c, project) = env_mapper();
        for path in [
            project.clone(),
            project.join("src/main.c"),
            project.join("build/out/app.exe"),
            drive_c.clone(),
            drive_c.join("Temp/t.tmp"),
            drive_c.join("windows/system32/kernel32.dll"),
        ] {
            let win = mapper.to_windows(&path).unwrap();
            assert_eq!(mapper.to_linux(&win).unwrap(), path, "via {win}");
        }
    }

    #[test]
    fn windows_round_trip_is_canonical() {
        let (mapper, _, _) = env_mapper();
        for win in ["C:\\", "C:\\Temp", "C:\\src\\demo\\a b\\x.c"] {
            let linux = mapper.to_linux(win).unwrap();
            assert_eq!(mapper.to_windows(&linux).unwrap(), win);
        }
    }

    #[test]
    fn mapping_windows_side_is_canonicalized() {
        let mapper = PathMapper::new(vec![Mapping {
            linux: PathBuf::from("/data"),
            windows: "c:/data/".to_owned(),
        }]);
        assert_eq!(mapper.mappings()[0].windows, "C:\\data");
        assert_eq!(
            mapper.to_windows(Path::new("/data/x")).unwrap(),
            "C:\\data\\x"
        );
        assert_eq!(
            mapper.to_linux("C:\\data\\x").unwrap(),
            PathBuf::from("/data/x")
        );
    }

    #[test]
    fn empty_mapper_maps_nothing() {
        let mapper = PathMapper::new(Vec::new());
        assert!(matches!(
            mapper.to_windows(Path::new("/x")).unwrap_err(),
            PathError::Unmapped { .. }
        ));
        assert!(matches!(
            mapper.to_linux("C:\\x").unwrap_err(),
            PathError::Unmapped { .. }
        ));
    }

    #[test]
    fn to_windows_normalizes_dotdot_instead_of_escaping() {
        let mapper = PathMapper::for_environment(
            Path::new("/env/drive_c"),
            Path::new("/home/alice/code/demo"),
            "demo",
        );
        let err = mapper
            .to_windows(Path::new("/home/alice/code/demo/../demo-secrets/key.pem"))
            .unwrap_err();
        assert!(matches!(err, PathError::Unmapped { .. }));
        assert_eq!(
            mapper
                .to_windows(Path::new("/home/alice/code/demo/build/../src/main.c"))
                .unwrap(),
            "C:\\src\\demo\\src\\main.c"
        );
        assert_eq!(
            mapper
                .to_windows(Path::new("/env/drive_c/Temp/../windows"))
                .unwrap(),
            "C:\\windows"
        );
    }

    #[test]
    fn normalize_lexically_stops_at_root() {
        assert_eq!(
            normalize_lexically(Path::new("/../../etc/passwd")),
            PathBuf::from("/etc/passwd")
        );
        assert_eq!(
            normalize_lexically(Path::new("/a/b/../..")),
            PathBuf::from("/")
        );
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
    }
}
