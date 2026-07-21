use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use lsw_config::Dirs;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Supported,
    Unsupported,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entry {
    pub supported_count: u64,
    pub unsupported_count: u64,
    #[serde(default)]
    pub last_runtime: String,
}

impl Entry {
    pub fn verdict(&self) -> Verdict {
        if self.supported_count > 0 {
            Verdict::Supported
        } else {
            Verdict::Unsupported
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatDb {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, Entry>,
}

fn one() -> u32 {
    1
}

impl CompatDb {
    fn path(dirs: &Dirs) -> PathBuf {
        dirs.data.join("compat-db.json")
    }

    pub fn load(dirs: &Dirs) -> Result<Self> {
        let path = Self::path(dirs);
        if !path.is_file() {
            return Ok(Self {
                version: 1,
                entries: BTreeMap::new(),
            });
        }
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(path.clone(), e))?;
        serde_json::from_str(&text).map_err(|e| Error::CompatDb {
            detail: format!("cannot parse {}: {e}", path.display()),
        })
    }

    pub fn save(&self, dirs: &Dirs) -> Result<()> {
        let path = Self::path(dirs);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent.to_path_buf(), e))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::CompatDb {
            detail: format!("cannot serialize compat db: {e}"),
        })?;
        std::fs::write(&path, text).map_err(|e| Error::io(path, e))
    }

    pub fn record(&mut self, runtime: &str, supported: &[String], unsupported: &[String]) {
        for key in supported {
            let e = self.entries.entry(normalize(key)).or_default();
            e.supported_count += 1;
            e.last_runtime = runtime.to_owned();
        }
        for key in unsupported {
            let e = self.entries.entry(normalize(key)).or_default();
            e.unsupported_count += 1;
            e.last_runtime = runtime.to_owned();
        }
    }

    pub fn query(&self, key: &str) -> Option<&Entry> {
        self.entries.get(&normalize(key))
    }
}

fn normalize(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(base: &std::path::Path) -> Dirs {
        Dirs {
            data: base.join("data"),
            config: base.join("cfg"),
            cache: base.join("cache"),
        }
    }

    #[test]
    fn record_query_and_persist_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());

        let mut db = CompatDb::load(&d).unwrap();
        db.record(
            "wine 11.13",
            &["kernel32.dll".into(), "KERNEL32.DLL".into()],
            &["d3d12.dll".into()],
        );
        db.save(&d).unwrap();

        let reloaded = CompatDb::load(&d).unwrap();
        let k = reloaded.query("Kernel32.dll").unwrap();
        assert_eq!(k.supported_count, 2);
        assert_eq!(k.verdict(), Verdict::Supported);

        let d3d = reloaded.query("d3d12.dll").unwrap();
        assert_eq!(d3d.verdict(), Verdict::Unsupported);
        assert_eq!(d3d.last_runtime, "wine 11.13");

        assert!(reloaded.query("never-seen.dll").is_none());
    }

    #[test]
    fn missing_db_loads_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let db = CompatDb::load(&dirs(tmp.path())).unwrap();
        assert!(db.entries.is_empty());
    }
}
