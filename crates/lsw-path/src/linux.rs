use std::path::PathBuf;

use crate::error::PathError;
use crate::types::{Mapping, PathMapper};
use crate::windows::parse_windows;

fn normalize<'a>(comps: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for &c in comps {
        match c {
            "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(c),
        }
    }
    out
}

impl PathMapper {
    pub fn to_linux(&self, path: &str) -> Result<PathBuf, PathError> {
        let (drive, raw_comps) = parse_windows(path)?;
        let comps = normalize(&raw_comps);
        let mut best: Option<(usize, &Mapping)> = None;
        for mapping in &self.mappings {
            let Ok((map_drive, map_raw)) = parse_windows(&mapping.windows) else {
                continue;
            };
            let map_comps = normalize(&map_raw);
            if map_drive == drive
                && comps.len() >= map_comps.len()
                && comps[..map_comps.len()]
                    .iter()
                    .zip(&map_comps)
                    .all(|(a, b)| a.to_lowercase() == b.to_lowercase())
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
