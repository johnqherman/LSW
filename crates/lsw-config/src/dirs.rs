use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};
use crate::manifest::{read_toml, write_toml};
use crate::types::TargetArch;
use crate::{ENVIRONMENT_FORMAT_VERSION, ENVIRONMENT_MANIFEST};

/// Persisted manifest describing a resolved environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentManifest {
    /// Environment name.
    pub name: String,
    /// Format version of this manifest.
    pub format: u32,
    /// Target architecture for this environment.
    pub target_arch: TargetArch,
    /// Resolved cross-compilation toolchain.
    pub toolchain: ResolvedToolchain,
    /// Resolved Windows runtime.
    pub runtime: ResolvedRuntime,
}

/// Resolved cross-compilation toolchain paths and flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedToolchain {
    /// Toolchain provider name.
    pub provider: String,
    /// Toolchain version string.
    pub version: String,
    /// Path to the C compiler.
    pub cc: PathBuf,
    /// Path to the C++ compiler.
    pub cxx: PathBuf,
    /// Sysroot directory for cross headers and libraries.
    pub sysroot: PathBuf,
    /// Extra flags passed to the C compiler.
    #[serde(default)]
    pub c_flags: Vec<String>,
    /// Extra flags passed to the C++ compiler.
    #[serde(default)]
    pub cxx_flags: Vec<String>,
    /// Extra flags passed to the linker.
    #[serde(default)]
    pub link_flags: Vec<String>,
}

/// Resolved Windows runtime executable and version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRuntime {
    /// Runtime provider name.
    pub provider: String,
    /// Runtime version string.
    pub version: String,
    /// Path to the runtime executable.
    pub executable: PathBuf,
}

impl EnvironmentManifest {
    /// Loads and validates an environment manifest from disk.
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

    /// Writes this manifest to disk as TOML.
    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "env.toml")
    }
}

/// Per-user configuration loaded from `~/.config/lsw/config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    /// Default environment name to use when none is specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_environment: Option<String>,
}

impl UserConfig {
    /// Loads user config from the default path, returning defaults if absent.
    pub fn load_default() -> Result<Self> {
        let path = Dirs::resolve()?.user_config_file();
        if path.is_file() {
            read_toml(&path)
        } else {
            Ok(Self::default())
        }
    }
}

/// XDG-based directory layout for LSW data, config, and cache.
#[derive(Debug, Clone, PartialEq)]
pub struct Dirs {
    /// Data directory (`$XDG_DATA_HOME/lsw`).
    pub data: PathBuf,
    /// Config directory (`$XDG_CONFIG_HOME/lsw`).
    pub config: PathBuf,
    /// Cache directory (`$XDG_CACHE_HOME/lsw`).
    pub cache: PathBuf,
}

impl Dirs {
    /// Resolves LSW directories from XDG base paths.
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

    /// Returns the directory containing all environments.
    pub fn environments(&self) -> PathBuf {
        self.data.join("environments")
    }

    /// Returns the directory for a named environment.
    pub fn environment(&self, name: &str) -> PathBuf {
        self.environments().join(sanitize_component(name))
    }

    /// Returns the directory containing cross-compilation sysroots.
    pub fn sysroots(&self) -> PathBuf {
        self.data.join("sysroots")
    }

    /// Returns the directory containing installed runtimes.
    pub fn runtimes(&self) -> PathBuf {
        self.data.join("runtimes")
    }

    /// Returns the directory containing installed toolchains.
    pub fn toolchains(&self) -> PathBuf {
        self.data.join("toolchains")
    }

    /// Returns the directory containing downloaded packages.
    pub fn packages(&self) -> PathBuf {
        self.data.join("packages")
    }

    /// Returns the directory containing managed Wine installations.
    pub fn wines(&self) -> PathBuf {
        self.data.join("wine")
    }

    /// Returns all managed data subdirectories.
    pub fn managed_dirs(&self) -> [PathBuf; 6] {
        [
            self.environments(),
            self.sysroots(),
            self.runtimes(),
            self.toolchains(),
            self.packages(),
            self.wines(),
        ]
    }

    /// Returns the directory for a named sysroot.
    pub fn sysroot(&self, name: &str) -> PathBuf {
        self.sysroots().join(sanitize_component(name))
    }

    /// Returns the path to the user config file.
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

/// Filesystem layout within a single environment directory.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentLayout {
    /// Root directory of this environment.
    pub root: PathBuf,
}

impl EnvironmentLayout {
    /// Creates a layout rooted at the given directory.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Path to the environment manifest file.
    pub fn manifest(&self) -> PathBuf {
        self.root.join(ENVIRONMENT_MANIFEST)
    }

    /// Wine prefix directory.
    pub fn prefix(&self) -> PathBuf {
        self.root.join("prefix")
    }

    /// `C:\` drive root inside the Wine prefix.
    pub fn drive_c(&self) -> PathBuf {
        self.prefix().join("drive_c")
    }

    /// Source mount point inside the Wine prefix.
    pub fn src(&self) -> PathBuf {
        self.drive_c().join("src")
    }

    /// Temporary directory inside the Wine prefix.
    pub fn temp(&self) -> PathBuf {
        self.drive_c().join("Temp")
    }

    /// Log output directory.
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Path to the generated `CMake` toolchain file.
    pub fn cmake_toolchain_file(&self) -> PathBuf {
        self.root.join("toolchain.cmake")
    }
}
