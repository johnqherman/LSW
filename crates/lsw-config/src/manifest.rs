use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::PROJECT_MANIFEST;
use crate::error::{ConfigError, Result};
use crate::types::{CaseSensitivity, LinkMode, TargetArch};

/// Top-level project manifest (`lsw.toml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    /// Project identity.
    pub project: ProjectSection,
    /// Target platform settings.
    #[serde(default, skip_serializing_if = "is_default_section")]
    pub target: TargetSection,
    /// Cross-compilation toolchain settings.
    #[serde(default, skip_serializing_if = "is_default_section")]
    pub toolchain: ToolchainSection,
    /// Windows runtime settings.
    #[serde(default, skip_serializing_if = "is_default_section")]
    pub runtime: RuntimeSection,
    /// Default environment preferences.
    #[serde(default, skip_serializing_if = "EnvironmentSection::is_empty")]
    pub environment: EnvironmentSection,
    /// Filesystem validation settings.
    #[serde(default, skip_serializing_if = "is_default_section")]
    pub filesystem: FilesystemSection,
    /// Custom build command override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<CommandSection>,
    /// Custom test command override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<CommandSection>,
    /// Sandbox resource limits and network policy.
    #[serde(default, skip_serializing_if = "is_default_section")]
    pub sandbox: SandboxSection,
    /// Native Windows verification settings.
    #[serde(default, skip_serializing_if = "VerifySection::is_empty")]
    pub verify: VerifySection,
    /// Environment variables and secrets.
    #[serde(default, skip_serializing_if = "EnvSection::is_empty")]
    pub env: EnvSection,
    /// Windows registry seed entries.
    #[serde(default, skip_serializing_if = "RegistrySection::is_empty")]
    pub registry: RegistrySection,
    /// Installer packaging settings.
    #[serde(default, skip_serializing_if = "PackageSection::is_empty")]
    pub package: PackageSection,
    /// Third-party dependency version pins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

/// Installer packaging metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSection {
    /// Application version string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Publisher name for installer metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Short description for installer metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Path to an icon file (.ico).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// MSI/MSIX upgrade code GUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_code: Option<String>,
    /// Whether to create start-menu shortcuts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<bool>,
    /// URL shown in Add/Remove Programs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer_url: Option<String>,
    /// Whether the application is DPI-aware.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi_aware: Option<bool>,
    /// Whether the installer requires admin elevation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_admin: Option<bool>,
}

impl PackageSection {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Windows registry seed entries applied at environment creation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySection {
    /// Registry values to seed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seed: Vec<RegistrySeed>,
}

impl RegistrySection {
    fn is_empty(&self) -> bool {
        self.seed.is_empty()
    }
}

/// A single registry value to seed during environment creation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySeed {
    /// Registry key path (e.g. `HKCU\Software\App`).
    pub key: String,
    /// Value name.
    pub name: String,
    /// Value data.
    pub value: String,
    /// Value type (defaults to `"string"`).
    #[serde(default = "default_registry_type", rename = "type")]
    pub kind: String,
}

fn default_registry_type() -> String {
    "string".to_owned()
}

/// Environment variables and host-mapped secrets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvSection {
    /// Static environment variables.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, String>,
    /// Host environment variables mapped as secrets.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secret: BTreeMap<String, String>,
}

impl EnvSection {
    fn is_empty(&self) -> bool {
        self.vars.is_empty() && self.secret.is_empty()
    }
}

/// Native Windows verification settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VerifySection {
    /// Transport protocol (ssh, winrm, https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Remote host address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Remote working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_dir: Option<String>,
    /// SSH identity file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// Remote crash dump directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dump_dir: Option<String>,
}

impl VerifySection {
    fn is_empty(&self) -> bool {
        self.transport.is_none()
            && self.host.is_none()
            && self.remote_dir.is_none()
            && self.identity_file.is_none()
            && self.dump_dir.is_none()
    }
}

/// Sandbox resource limits and network isolation policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSection {
    /// Network access mode (host, isolated, none).
    #[serde(default = "default_sandbox_network")]
    pub network: String,
    /// CPU time limit in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_seconds: Option<u64>,
    /// Memory limit in megabytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
}

fn default_sandbox_network() -> String {
    "host".to_owned()
}

impl Default for SandboxSection {
    fn default() -> Self {
        Self {
            network: default_sandbox_network(),
            cpu_seconds: None,
            memory_mb: None,
        }
    }
}

/// Project identity section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    /// Project name used in paths and installer metadata.
    pub name: String,
}

/// Target platform section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSection {
    /// Target operating system (always `"windows"`).
    #[serde(default = "default_target_os")]
    pub os: String,
    /// Target CPU architecture.
    #[serde(default = "default_target_arch")]
    pub arch: TargetArch,
    /// Minimum Windows API version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
}

fn default_target_os() -> String {
    "windows".to_owned()
}

fn default_target_arch() -> TargetArch {
    TargetArch::X86_64
}

impl Default for TargetSection {
    fn default() -> Self {
        Self {
            os: default_target_os(),
            arch: default_target_arch(),
            api: None,
        }
    }
}

/// Cross-compilation toolchain preferences.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainSection {
    /// Toolchain provider name (e.g. `llvm-mingw`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Toolchain version constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Static or dynamic linking preference.
    #[serde(default)]
    pub link: LinkMode,
    /// Enable `NativeAOT` compilation for .NET.
    #[serde(default, skip_serializing_if = "is_false")]
    pub aot: bool,
    /// Enable ccache for compilation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ccache: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_default_section<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Windows runtime provider settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSection {
    /// Runtime provider name (default: `"wine"`).
    #[serde(default = "default_runtime_provider")]
    pub provider: String,
    /// Runtime version constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

fn default_runtime_provider() -> String {
    "wine".to_owned()
}

impl Default for RuntimeSection {
    fn default() -> Self {
        Self {
            provider: default_runtime_provider(),
            version: None,
        }
    }
}

/// Default environment name preference.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSection {
    /// Preferred environment name for `lsw env create`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EnvironmentSection {
    fn is_empty(&self) -> bool {
        self.name.is_none()
    }
}

/// Filesystem validation settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FilesystemSection {
    /// Case-sensitivity mode for cross-compilation validation.
    #[serde(
        default,
        rename = "case",
        skip_serializing_if = "CaseSensitivity::is_default"
    )]
    pub case: CaseSensitivity,
}

/// A shell command sequence.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSection {
    /// Command and arguments.
    pub command: Vec<String>,
}

impl ProjectManifest {
    /// ```
    /// let m = lsw_config::ProjectManifest::new("hello-win32");
    /// assert_eq!(m.project.name, "hello-win32");
    /// ```
    pub fn new(name: &str) -> Self {
        Self {
            project: ProjectSection {
                name: name.to_owned(),
            },
            ..Self::default()
        }
    }

    /// Loads a project manifest from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        read_toml(path)
    }

    /// Writes this manifest to disk as TOML (atomic rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "lsw.toml")
    }

    /// Creates a new manifest file, failing if the file already exists.
    pub fn save_new(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|source| ConfigError::Serialize {
            what: "lsw.toml",
            source: Box::new(source),
        })?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(ConfigError::write(
                    path,
                    std::io::Error::other("parent directory is a symlink"),
                ));
            }
            fs::create_dir_all(parent).map_err(|e| ConfigError::write(path, e))?;
        }
        create_new_file(path, &text)
    }

    /// Walks upward from `start` to find and load the nearest `lsw.toml`.
    pub fn discover(start: &Path) -> Result<(PathBuf, Self)> {
        let mut dir = Some(start);
        while let Some(d) = dir {
            let candidate = d.join(PROJECT_MANIFEST);
            if candidate.is_file() {
                return Ok((d.to_path_buf(), Self::load(&candidate)?));
            }
            dir = d.parent();
        }
        Err(ConfigError::ProjectNotFound {
            start: start.to_path_buf(),
        })
    }
}

const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    use std::io::Read;
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(ConfigError::read(
            path,
            std::io::Error::other("refusing to read a symlinked config file"),
        ));
    }
    let file = fs::File::open(path).map_err(|e| ConfigError::read(path, e))?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ConfigError::read(path, e))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ConfigError::read(
            path,
            std::io::Error::other(format!("file exceeds {MAX_MANIFEST_BYTES}-byte limit")),
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| ConfigError::read(path, std::io::Error::other("file is not valid UTF-8")))?;
    toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

pub(crate) fn write_toml<T: Serialize>(path: &Path, value: &T, what: &'static str) -> Result<()> {
    let text = toml::to_string_pretty(value).map_err(|source| ConfigError::Serialize {
        what,
        source: Box::new(source),
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConfigError::write(path, e))?;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let uniq = format!(
        "{}.{nanos}.{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let tmp = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{ext}.{uniq}.tmp"),
        None => format!("{uniq}.tmp"),
    });
    create_new_file(&tmp, &text)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::write(path, e));
    }
    Ok(())
}

fn create_new_file(path: &Path, text: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| ConfigError::write(path, e))?;
    if let Err(e) = file.write_all(text.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(ConfigError::write(path, e));
    }
    Ok(())
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
