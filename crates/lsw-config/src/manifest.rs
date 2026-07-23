use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::PROJECT_MANIFEST;
use crate::error::{ConfigError, Result};
use crate::types::{CaseSensitivity, LinkMode, TargetArch};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub project: ProjectSection,
    #[serde(default)]
    pub target: TargetSection,
    #[serde(default)]
    pub toolchain: ToolchainSection,
    #[serde(default)]
    pub runtime: RuntimeSection,
    #[serde(default, skip_serializing_if = "EnvironmentSection::is_empty")]
    pub environment: EnvironmentSection,
    #[serde(default)]
    pub filesystem: FilesystemSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<CommandSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<CommandSection>,
    #[serde(default)]
    pub sandbox: SandboxSection,
    #[serde(default, skip_serializing_if = "VerifySection::is_empty")]
    pub verify: VerifySection,
    #[serde(default, skip_serializing_if = "EnvSection::is_empty")]
    pub env: EnvSection,
    #[serde(default, skip_serializing_if = "RegistrySection::is_empty")]
    pub registry: RegistrySection,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seed: Vec<RegistrySeed>,
}

impl RegistrySection {
    fn is_empty(&self) -> bool {
        self.seed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySeed {
    pub key: String,
    pub name: String,
    pub value: String,
    #[serde(default = "default_registry_type", rename = "type")]
    pub kind: String,
}

fn default_registry_type() -> String {
    "string".to_owned()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvSection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secret: BTreeMap<String, String>,
}

impl EnvSection {
    fn is_empty(&self) -> bool {
        self.vars.is_empty() && self.secret.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct VerifySection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSection {
    #[serde(default = "default_sandbox_network")]
    pub network: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_seconds: Option<u64>,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSection {
    #[serde(default = "default_target_os")]
    pub os: String,
    #[serde(default = "default_target_arch")]
    pub arch: TargetArch,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub link: LinkMode,
    #[serde(default, skip_serializing_if = "is_false")]
    pub aot: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSection {
    #[serde(default = "default_runtime_provider")]
    pub provider: String,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

impl EnvironmentSection {
    fn is_empty(&self) -> bool {
        self.name.is_none() && self.profile.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemSection {
    #[serde(default = "default_project_drive")]
    pub project_drive: String,
    #[serde(default = "default_mount_project")]
    pub mount_project: String,
    #[serde(
        default,
        rename = "case",
        skip_serializing_if = "CaseSensitivity::is_default"
    )]
    pub case: CaseSensitivity,
}

fn default_project_drive() -> String {
    "C:".to_owned()
}

fn default_mount_project() -> String {
    "/src".to_owned()
}

impl Default for FilesystemSection {
    fn default() -> Self {
        Self {
            project_drive: default_project_drive(),
            mount_project: default_mount_project(),
            case: CaseSensitivity::Native,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSection {
    pub command: Vec<String>,
}

impl ProjectManifest {
    pub fn new(name: &str) -> Self {
        Self {
            project: ProjectSection {
                name: name.to_owned(),
            },
            ..Self::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        read_toml(path)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_toml(path, self, "lsw.toml")
    }

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

pub(crate) fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| ConfigError::read(path, e))?;
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
    let tmp = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{ext}.tmp"),
        None => "tmp".to_owned(),
    });
    fs::write(&tmp, text).map_err(|e| ConfigError::write(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| ConfigError::write(path, e))
}
