use std::fmt;
use std::path::{Path, PathBuf};

/// Errors from PE parsing and inspection.
#[derive(Debug, thiserror::Error)]
pub enum PeError {
    #[error(
        "LSW1301: cannot read {}: {source}; check that the file exists and is readable",
        path.display()
    )]
    /// I/O error reading the PE file.
    Io {
        /// Path to the file.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    #[error(
        "LSW1302: {} has an MZ header but is not a valid PE image ({detail}); \
         the file is likely truncated or corrupted - rebuild it or restore it from source",
        path.display()
    )]
    /// File has an MZ header but is not a valid PE image.
    MalformedPe {
        /// Path to the file.
        path: PathBuf,
        /// Description of the parse failure.
        detail: String,
    },
    #[error(
        "LSW1303: {} is not a PE executable; pass a Windows binary (.exe/.dll) \
         such as one produced by `lsw build`",
        path.display()
    )]
    /// File is not a PE executable at all.
    NotPe {
        /// Path to the file.
        path: PathBuf,
    },
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

pub(crate) const MAX_PE_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn read_pe(path: &Path) -> Result<Vec<u8>, PeError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| PeError::io(path, e))?;
    if let Ok(meta) = file.metadata()
        && meta.len() > MAX_PE_BYTES
    {
        return Err(PeError::malformed(
            path,
            format!("file exceeds {MAX_PE_BYTES}-byte limit for PE parsing"),
        ));
    }
    let mut data = Vec::new();
    file.take(MAX_PE_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| PeError::io(path, e))?;
    if data.len() as u64 > MAX_PE_BYTES {
        return Err(PeError::malformed(
            path,
            format!("file exceeds {MAX_PE_BYTES}-byte limit for PE parsing"),
        ));
    }
    Ok(data)
}
