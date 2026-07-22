use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
pub struct Delta {
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DiffReport {
    pub imports: Delta,
    pub exports: Delta,
}

fn delta(old: &[String], new: &[String]) -> Delta {
    let old: BTreeSet<&String> = old.iter().collect();
    let new: BTreeSet<&String> = new.iter().collect();
    Delta {
        added: new.difference(&old).map(|s| (*s).clone()).collect(),
        removed: old.difference(&new).map(|s| (*s).clone()).collect(),
    }
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

pub fn diff(a: &Path, b: &Path) -> Result<DiffReport> {
    require_file(a)?;
    require_file(b)?;
    Ok(DiffReport {
        imports: delta(&lsw_pe::imports(a)?, &lsw_pe::imports(b)?),
        exports: delta(&lsw_pe::exports(a)?, &lsw_pe::exports(b)?),
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
}
