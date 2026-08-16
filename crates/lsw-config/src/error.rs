use std::path::{Path, PathBuf};

/// Configuration errors from manifest parsing, I/O, or version checks.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read a configuration file.
    #[error("LSW1001: cannot read {}: {source}", path.display())]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to write a configuration file.
    #[error("LSW1002: cannot write {}: {source}", path.display())]
    Write {
        /// Path that could not be written.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// TOML syntax or schema error in a configuration file.
    #[error("LSW1003: invalid TOML in {}: {source}", path.display())]
    Parse {
        /// Path containing invalid TOML.
        path: PathBuf,
        /// Parse error details.
        source: Box<toml::de::Error>,
    },
    /// Failed to serialize a configuration value to TOML.
    #[error("LSW1004: cannot serialize {what}: {source}")]
    Serialize {
        /// Description of what was being serialized.
        what: &'static str,
        /// Serialization error details.
        source: Box<toml::ser::Error>,
    },
    /// No `lsw.toml` found in the directory tree.
    #[error(
        "LSW1005: no lsw.toml found in {} or any parent directory\n\
         Possible fixes:\n  lsw init  (scaffold a project here)\n  cd into an existing LSW project", start.display()
    )]
    ProjectNotFound {
        /// Directory where the search started.
        start: PathBuf,
    },
    /// Home directory could not be determined.
    #[error("LSW1006: cannot determine home directory; set $HOME to a writable directory")]
    NoHome,
    /// File was created by a newer LSW version.
    #[error(
        "LSW1007: {} was created by a newer LSW (format {found}, this build supports {supported}); upgrade LSW or recreate the environment", path.display()
    )]
    UnsupportedFormat {
        /// Path to the file with an unsupported format version.
        path: PathBuf,
        /// Format version found in the file.
        found: u32,
        /// Maximum format version this build supports.
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

/// Result type using [`ConfigError`].
pub type Result<T> = std::result::Result<T, ConfigError>;
