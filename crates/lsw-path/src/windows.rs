use std::path::{Component, Path, PathBuf};

use crate::error::PathError;
use crate::types::PathMapper;

impl PathMapper {
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
                        if part.contains('\\') {
                            return Err(PathError::Unmapped {
                                path: path.to_string_lossy().into_owned(),
                            });
                        }
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
}

pub(crate) fn normalize_lexically(path: &Path) -> PathBuf {
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

pub(crate) fn parse_windows(path: &str) -> Result<(char, Vec<&str>), PathError> {
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

pub(crate) fn render_windows(drive: char, comps: &[impl AsRef<str>]) -> String {
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
