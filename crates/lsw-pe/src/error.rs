use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PeError {
    #[error(
        "LSW1301: cannot read {}: {source}; check that the file exists and is readable",
        path.display()
    )]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "LSW1302: {} has an MZ header but is not a valid PE image ({detail}); \
         the file is likely truncated or corrupted - rebuild it or restore it from source",
        path.display()
    )]
    MalformedPe { path: PathBuf, detail: String },
    #[error(
        "LSW1303: {} is not a PE executable; pass a Windows binary (.exe/.dll) \
         such as one produced by `lsw build`",
        path.display()
    )]
    NotPe { path: PathBuf },
}

impl PeError {
    pub(crate) fn io(path: &Path, source: std::io::Error) -> Self {
        PeError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn malformed(path: &Path, detail: impl fmt::Display) -> Self {
        PeError::MalformedPe {
            path: path.to_path_buf(),
            detail: detail.to_string(),
        }
    }
}
