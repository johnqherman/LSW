use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
pub struct Delta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SectionResize {
    pub name: String,
    pub raw_size_delta: i64,
}

#[derive(Debug, Serialize)]
pub struct DiffReport {
    pub imports: Delta,
    pub exports: Delta,
    pub sections: Delta,
    pub resized: Vec<SectionResize>,
    pub size_delta: i64,
}

fn delta(old: &[String], new: &[String]) -> Delta {
    let old: BTreeSet<&String> = old.iter().collect();
    let new: BTreeSet<&String> = new.iter().collect();
    Delta {
        added: new.difference(&old).map(|s| (*s).clone()).collect(),
        removed: old.difference(&new).map(|s| (*s).clone()).collect(),
    }
}

pub(crate) fn keyed_sizes(sections: &[lsw_pe::SectionInfo]) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    for s in sections {
        *out.entry(s.name.clone()).or_default() += u64::from(s.raw_size);
    }
    out
}

pub(crate) fn keyed_delta(
    old: &BTreeMap<String, u64>,
    new: &BTreeMap<String, u64>,
) -> (Delta, Vec<SectionResize>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut resized = Vec::new();
    for (name, size) in new {
        match old.get(name) {
            None => added.push(name.clone()),
            Some(before) if before != size => resized.push(SectionResize {
                name: name.clone(),
                raw_size_delta: *size as i64 - *before as i64,
            }),
            Some(_) => {}
        }
    }
    for name in old.keys() {
        if !new.contains_key(name) {
            removed.push(name.clone());
        }
    }
    (Delta { added, removed }, resized)
}

fn require_file(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        })
    }
}

fn file_len(path: &Path) -> Result<u64> {
    std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| Error::io(path.to_path_buf(), e))
}

pub fn diff(a: &Path, b: &Path) -> Result<DiffReport> {
    require_file(a)?;
    require_file(b)?;
    let old = lsw_pe::PeImage::open(a)?;
    let new = lsw_pe::PeImage::open(b)?;
    let old_sections = keyed_sizes(&old.details()?.sections);
    let new_sections = keyed_sizes(&new.details()?.sections);
    let (sections, resized) = keyed_delta(&old_sections, &new_sections);
    Ok(DiffReport {
        imports: delta(&old.imports()?, &new.imports()?),
        exports: delta(&old.exports()?, &new.exports()?),
        sections,
        resized,
        size_delta: file_len(b)? as i64 - file_len(a)? as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_reports_additions_and_removals() {
        let old = vec!["a".to_owned(), "b".to_owned()];
        let new = vec!["b".to_owned(), "c".to_owned()];
        let d = delta(&old, &new);
        assert_eq!(d.added, vec!["c"]);
        assert_eq!(d.removed, vec!["a"]);
    }

    #[test]
    fn keyed_delta_reports_added_removed_and_resized() {
        let old = BTreeMap::from([
            (".text".to_owned(), 4096u64),
            (".gone".to_owned(), 512u64),
            (".data".to_owned(), 1024u64),
        ]);
        let new = BTreeMap::from([
            (".text".to_owned(), 8192u64),
            (".data".to_owned(), 1024u64),
            (".fresh".to_owned(), 256u64),
        ]);
        let (d, resized) = keyed_delta(&old, &new);
        assert_eq!(d.added, vec![".fresh"]);
        assert_eq!(d.removed, vec![".gone"]);
        assert_eq!(resized.len(), 1);
        assert_eq!(resized[0].name, ".text");
        assert_eq!(resized[0].raw_size_delta, 4096);
    }

    #[test]
    fn keyed_sizes_sums_duplicate_section_names() {
        let sections = vec![
            lsw_pe::SectionInfo {
                name: ".text".into(),
                virtual_size: 10,
                raw_size: 100,
                raw_offset: 0,
            },
            lsw_pe::SectionInfo {
                name: ".text".into(),
                virtual_size: 10,
                raw_size: 50,
                raw_offset: 100,
            },
        ];
        assert_eq!(keyed_sizes(&sections)[".text"], 150);
    }
}
