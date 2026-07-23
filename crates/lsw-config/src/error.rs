use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("LSW1001: cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("LSW1002: cannot write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("LSW1003: invalid TOML in {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("LSW1004: cannot serialize {what}: {source}")]
    Serialize {
        what: &'static str,
        source: Box<toml::ser::Error>,
    },
    #[error(
        "LSW1005: no lsw.toml found in {} or any parent directory\n\
         Possible fixes:\n  lsw init  (scaffold a project here)\n  cd into an existing LSW project", start.display()
    )]
    ProjectNotFound { start: PathBuf },
    #[error("LSW1006: cannot determine home directory; set $HOME to a writable directory")]
    NoHome,
    #[error(
        "LSW1007: {} was created by a newer LSW (format {found}, this build supports {supported}); upgrade LSW or recreate the environment", path.display()
    )]
    UnsupportedFormat {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
}

impl ConfigError {
    pub(crate) fn read(path: &Path, source: std::io::Error) -> Self {
        ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn write(path: &Path, source: std::io::Error) -> Self {
        ConfigError::Write {
            path: path.to_path_buf(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, ConfigError>;
