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
    /// ```
    /// use lsw_path::{Mapping, PathMapper};
    /// use std::path::PathBuf;
    ///
    /// let mapper = PathMapper::new(vec![Mapping {
    ///     linux: PathBuf::from("/data/drive_c"),
    ///     windows: "C:\\".to_owned(),
    /// }]);
    /// assert_eq!(mapper.mappings().len(), 1);
    /// ```
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

    /// ```
    /// use lsw_path::PathMapper;
    /// use std::path::Path;
    ///
    /// let m = PathMapper::for_environment(
    ///     Path::new("/env/drive_c"),
    ///     Path::new("/home/alice/demo"),
    ///     "demo",
    /// );
    /// assert_eq!(m.mappings().len(), 2);
    /// ```
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
