use std::path::PathBuf;

/// Errors from path translation between Linux and Windows forms.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error(
        "LSW1201: path '{}' is not absolute; path translation requires absolute Linux paths - canonicalize it first (e.g. std::fs::canonicalize) or join it onto the project root",
        path.display()
    )]
    /// Input path is not absolute.
    NotAbsolute {
        /// Path that was not absolute.
        path: PathBuf,
    },
    #[error(
        "LSW1202: no mapping covers '{path}'; the path must live under a mapped root (the project root or the environment's drive_c) - move it under one of those or register an additional Mapping for its root"
    )]
    /// No mapping covers the given path.
    Unmapped {
        /// Path with no mapping.
        path: String,
    },
    #[error(
        "LSW1203: '{path}' is not a drive-letter Windows path; expected a form like C:\\dir\\file (forward slashes and a lowercase drive letter are accepted) - check that the path came from to_windows or a Windows-side tool"
    )]
    /// Not a valid drive-letter Windows path.
    InvalidWindowsPath {
        /// Path that is not a valid drive-letter Windows path.
        path: String,
    },
    #[error(
        "LSW1204: path '{}' contains a non-UTF-8 component that cannot be represented as a Windows path; rename the offending file or directory to valid UTF-8",
        path.display()
    )]
    /// Path contains non-UTF-8 bytes.
    NonUtf8 {
        /// Path containing non-UTF-8 bytes.
        path: PathBuf,
    },
}
