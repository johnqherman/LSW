use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::manifest::{read_toml, write_toml};
use crate::types::TargetArch;
use crate::{ENVIRONMENT_FORMAT_VERSION, LOCKFILE_VERSION};

/// Lockfile pinning exact toolchain, runtime, and dependency versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    /// Lockfile format version.
    pub version: u32,
    /// Environment format version at lock time.
    pub environment_format: u32,
    /// Target architecture.
    pub target_arch: TargetArch,
    /// Locked toolchain identity and hash.
    pub toolchain: LockedComponent,
    /// Locked runtime identity and hash.
    pub runtime: LockedComponent,
    /// Locked sysroot identity and hash.
    pub sysroot: LockedComponent,
    /// Locked dependency versions keyed by package name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, LockedDep>,
}

/// A locked dependency with version and integrity hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedDep {
    /// Package version string.
    pub version: String,
    /// SHA-256 integrity hash.
    pub sha256: String,
}

/// A locked component (toolchain, runtime, or sysroot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedComponent {
    /// Provider name.
    pub provider: String,
    /// Provider version string.
    pub version: String,
    /// SHA-256 integrity hash.
    pub sha256: String,
}

impl Lockfile {
    /// Loads and validates a lockfile from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let lock: Self = read_toml(path)?;
        let newer = lock.version.max(lock.environment_format);
        if lock.version > LOCKFILE_VERSION || lock.environment_format > ENVIRONMENT_FORMAT_VERSION {
            return Err(ConfigError::UnsupportedFormat {
                path: path.to_path_buf(),
                found: newer,
                supported: LOCKFILE_VERSION.max(ENVIRONMENT_FORMAT_VERSION),
            });
        }
        Ok(lock)
    }

    /// Writes this lockfile to disk as TOML.
    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "lsw.lock")
    }
}
