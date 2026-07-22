use std::path::PathBuf;

use crate::error::PathError;
use crate::types::{Mapping, PathMapper};
use crate::windows::parse_windows;

impl PathMapper {
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
