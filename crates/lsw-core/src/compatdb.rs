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
        let empty = || Self {
            version: 1,
            entries: BTreeMap::new(),
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
            Err(e) => return Err(Error::io(path.clone(), e)),
        };
        match serde_json::from_str(&text) {
            Ok(db) => Ok(db),
            Err(e) => {
                let backup = path.with_extension("json.corrupt");
                let _ = std::fs::rename(&path, &backup);
                tracing::warn!(
                    "compat database at {} was unreadable ({e}); backed up to {} and starting fresh",
                    path.display(),
                    backup.display()
                );
                Ok(empty())
            }
        }
    }

    pub fn save(&self, dirs: &Dirs) -> Result<()> {
        let path = Self::path(dirs);
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir.to_path_buf(), e))?;
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::CompatDb {
            detail: format!("cannot serialize compat db: {e}"),
        })?;
        let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
        std::fs::write(&tmp, text).map_err(|e| Error::io(tmp.clone(), e))?;
        std::fs::rename(&tmp, &path).map_err(|e| Error::io(path, e))
    }

    pub fn record(&mut self, runtime: &str, supported: &[String], unsupported: &[String]) {
        for key in supported {
            let e = self.entries.entry(normalize(key)).or_default();
            e.supported_count = e.supported_count.saturating_add(1);
            e.last_runtime = runtime.to_owned();
        }
        for key in unsupported {
            let e = self.entries.entry(normalize(key)).or_default();
            e.unsupported_count = e.unsupported_count.saturating_add(1);
            e.last_runtime = runtime.to_owned();
        }
    }

    pub fn lock(dirs: &Dirs) -> Result<DbLock> {
        DbLock::acquire(dirs)
    }

    pub fn query(&self, key: &str) -> Option<&Entry> {
        self.entries.get(&normalize(key))
    }
}

fn normalize(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

pub struct DbLock {
    _file: std::fs::File,
}

impl DbLock {
    fn acquire(dirs: &Dirs) -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        std::fs::create_dir_all(&dirs.data).map_err(|e| Error::io(dirs.data.clone(), e))?;
        let lock_path = dirs.data.join("compat-db.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| Error::io(lock_path.clone(), e))?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(Error::CompatDb {
                detail: format!(
                    "cannot lock {}: {}",
                    lock_path.display(),
                    std::io::Error::last_os_error()
                ),
            });
        }
        Ok(Self { _file: file })
    }
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

    #[test]
    fn corrupt_db_degrades_and_backs_up() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        std::fs::create_dir_all(&d.data).unwrap();
        let path = CompatDb::path(&d);
        std::fs::write(&path, b"{ this is not json").unwrap();

        let db = CompatDb::load(&d).unwrap();
        assert!(db.entries.is_empty());
        assert!(path.with_extension("json.corrupt").is_file());
        let mut db = db;
        db.record("wine 11", &["a.dll".into()], &[]);
        db.save(&d).unwrap();
        assert!(CompatDb::load(&d).unwrap().query("a.dll").is_some());
    }

    #[test]
    fn concurrent_records_do_not_lose_updates() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        std::fs::create_dir_all(&d.data).unwrap();

        let one = |key: &str| {
            let _g = CompatDb::lock(&d).unwrap();
            let mut db = CompatDb::load(&d).unwrap();
            db.record("wine", &[key.to_owned()], &[]);
            db.save(&d).unwrap();
        };
        let d2 = d.clone();
        let t = std::thread::spawn(move || {
            for i in 0..20 {
                let _g = CompatDb::lock(&d2).unwrap();
                let mut db = CompatDb::load(&d2).unwrap();
                db.record("wine", &[format!("b{i}.dll")], &[]);
                db.save(&d2).unwrap();
            }
        });
        for i in 0..20 {
            one(&format!("a{i}.dll"));
        }
        t.join().unwrap();

        let db = CompatDb::load(&d).unwrap();
        assert_eq!(db.entries.len(), 40);
    }
}
