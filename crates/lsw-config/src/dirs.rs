use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::manifest::{read_toml, write_toml};
use crate::types::TargetArch;
use crate::{ENVIRONMENT_FORMAT_VERSION, ENVIRONMENT_MANIFEST};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentManifest {
    pub name: String,
    pub format: u32,
    pub target_arch: TargetArch,
    pub toolchain: ResolvedToolchain,
    pub runtime: ResolvedRuntime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedToolchain {
    pub provider: String,
    pub version: String,
    pub cc: PathBuf,
    pub cxx: PathBuf,
    pub sysroot: PathBuf,
    #[serde(default)]
    pub c_flags: Vec<String>,
    #[serde(default)]
    pub cxx_flags: Vec<String>,
    #[serde(default)]
    pub link_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRuntime {
    pub provider: String,
    pub version: String,
    pub executable: PathBuf,
}

impl EnvironmentManifest {
    pub fn load(path: &Path) -> Result<Self> {
        let manifest: Self = read_toml(path)?;
        if manifest.format > ENVIRONMENT_FORMAT_VERSION {
            return Err(ConfigError::UnsupportedFormat {
                path: path.to_path_buf(),
                found: manifest.format,
                supported: ENVIRONMENT_FORMAT_VERSION,
            });
        }
        Ok(manifest)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "env.toml")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_environment: Option<String>,
}

impl UserConfig {
    pub fn load_default() -> Result<Self> {
        let path = Dirs::resolve()?.user_config_file();
        if path.is_file() {
            read_toml(&path)
        } else {
            Ok(Self::default())
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dirs {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    pub fn resolve() -> Result<Self> {
        let data = dirs::data_dir().ok_or(ConfigError::NoHome)?.join("lsw");
        let config = dirs::config_dir().ok_or(ConfigError::NoHome)?.join("lsw");
        let cache = dirs::cache_dir().ok_or(ConfigError::NoHome)?.join("lsw");
        Ok(Self {
            data,
            config,
            cache,
        })
    }

    pub fn environments(&self) -> PathBuf {
        self.data.join("environments")
    }

    pub fn environment(&self, name: &str) -> PathBuf {
        self.environments().join(sanitize_component(name))
    }

    pub fn sysroots(&self) -> PathBuf {
        self.data.join("sysroots")
    }

    pub fn runtimes(&self) -> PathBuf {
        self.data.join("runtimes")
    }

    pub fn toolchains(&self) -> PathBuf {
        self.data.join("toolchains")
    }

    pub fn packages(&self) -> PathBuf {
        self.data.join("packages")
    }

    pub fn managed_dirs(&self) -> [PathBuf; 5] {
        [
            self.environments(),
            self.sysroots(),
            self.runtimes(),
            self.toolchains(),
            self.packages(),
        ]
    }

    pub fn sysroot(&self, name: &str) -> PathBuf {
        self.sysroots().join(sanitize_component(name))
    }

    pub fn user_config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }
}

fn sanitize_component(name: &str) -> &str {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return "_invalid_";
    }
    name
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentLayout {
    pub root: PathBuf,
}

impl EnvironmentLayout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn manifest(&self) -> PathBuf {
        self.root.join(ENVIRONMENT_MANIFEST)
    }

    pub fn prefix(&self) -> PathBuf {
        self.root.join("prefix")
    }

    pub fn drive_c(&self) -> PathBuf {
        self.prefix().join("drive_c")
    }

    pub fn src(&self) -> PathBuf {
        self.drive_c().join("src")
    }

    pub fn temp(&self) -> PathBuf {
        self.drive_c().join("Temp")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn cmake_toolchain_file(&self) -> PathBuf {
        self.root.join("toolchain.cmake")
    }
}
