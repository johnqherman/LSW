use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::manifest::{read_toml, write_toml};
use crate::types::TargetArch;

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
        read_toml(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "lsw.lock")
    }
}
