use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::manifest::{read_toml, write_toml};
use crate::types::TargetArch;
use crate::{ENVIRONMENT_FORMAT_VERSION, LOCKFILE_VERSION};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub version: u32,
    pub environment_format: u32,
    pub target_arch: TargetArch,
    pub toolchain: LockedComponent,
    pub runtime: LockedComponent,
    pub sysroot: LockedComponent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedComponent {
    pub provider: String,
    pub version: String,
    pub sha256: String,
}

impl Lockfile {
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

    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "lsw.lock")
    }
}
