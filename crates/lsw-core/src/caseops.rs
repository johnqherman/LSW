use std::collections::BTreeMap;
use std::path::Path;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "build",
    "zig-out",
    "bin",
    "obj",
    "node_modules",
    "verify-dumps",
    "deps",
    ".vscode",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseHazard {
    pub dir: String,
    pub names: Vec<String>,
}

pub fn hazards(root: &Path) -> Vec<CaseHazard> {
    let mut out = Vec::new();
    scan(root, root, &mut out);
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

fn scan(dir: &Path, root: &Path, out: &mut Vec<CaseHazard>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut folded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let is_dir = path.is_dir();
        folded
            .entry(name.to_lowercase())
            .or_default()
            .push(name.clone());
        if is_dir && !path.is_symlink() && !SKIP_DIRS.contains(&name.as_str()) {
            subdirs.push(path);
        }
    }
    for (_, mut names) in folded {
        if names.len() > 1 {
            names.sort();
            let rel = dir.strip_prefix(root).unwrap_or(dir);
            let label = rel.to_string_lossy();
            out.push(CaseHazard {
                dir: if label.is_empty() {
                    ".".to_owned()
                } else {
                    label.into_owned()
                },
                names,
            });
        }
    }
    for sub in subdirs {
        scan(&sub, root, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_case_insensitive_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("README.md"), b"a").unwrap();
        std::fs::write(tmp.path().join("readme.md"), b"b").unwrap();
        std::fs::write(tmp.path().join("unique.txt"), b"c").unwrap();
        let h = hazards(tmp.path());
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].names, vec!["README.md", "readme.md"]);
    }

    #[test]
    fn clean_tree_has_no_hazards() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.c"), b"a").unwrap();
        std::fs::write(tmp.path().join("util.c"), b"b").unwrap();
        assert!(hazards(tmp.path()).is_empty());
    }
}
