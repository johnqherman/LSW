use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::windows::{parse_windows, render_windows};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub linux: PathBuf,
    pub windows: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PathMapper {
    pub(crate) mappings: Vec<Mapping>,
}

impl PathMapper {
    pub fn new(mappings: Vec<Mapping>) -> Self {
        let mut mappings: Vec<Mapping> = mappings
            .into_iter()
            .filter_map(|mut m| {
                let (drive, comps) = parse_windows(&m.windows).ok()?;
                if comps.iter().any(|c| *c == "." || *c == "..") {
                    return None;
                }
                m.windows = render_windows(drive, &comps);
                Some(m)
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
}
