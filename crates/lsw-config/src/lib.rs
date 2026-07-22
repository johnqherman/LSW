use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PROJECT_MANIFEST: &str = "lsw.toml";
pub const PROJECT_LOCKFILE: &str = "lsw.lock";
pub const ENVIRONMENT_MANIFEST: &str = "env.toml";

pub const ENVIRONMENT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("LSW1001: cannot read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("LSW1002: cannot write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("LSW1003: invalid TOML in {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("LSW1004: cannot serialize {what}: {source}")]
    Serialize {
        what: &'static str,
        source: Box<toml::ser::Error>,
    },
    #[error("LSW1005: no lsw.toml found in {} or any parent directory", start.display())]
    ProjectNotFound { start: PathBuf },
    #[error("LSW1006: cannot determine home directory")]
    NoHome,
}

impl ConfigError {
    fn read(path: &Path, source: std::io::Error) -> Self {
        ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }
    }

    fn write(path: &Path, source: std::io::Error) -> Self {
        ConfigError::Write {
            path: path.to_path_buf(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, ConfigError>;

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
    #[serde(default)]
    pub environment: EnvironmentSection,
    #[serde(default)]
    pub filesystem: FilesystemSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<CommandSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<CommandSection>,
    #[serde(default)]
    pub sandbox: SandboxSection,
    #[serde(default)]
    pub verify: VerifySection,
    #[serde(default)]
    pub env: EnvSection,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvSection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secret: BTreeMap<String, String>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxSection {
    #[serde(default = "default_sandbox_network")]
    pub network: String,
}

fn default_sandbox_network() -> String {
    "host".to_owned()
}

impl Default for SandboxSection {
    fn default() -> Self {
        Self {
            network: default_sandbox_network(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetArch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "x86")]
    X86,
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "armv7")]
    Armv7,
    #[serde(rename = "arm64ec")]
    Arm64Ec,
}

impl TargetArch {
    pub fn mingw_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-w64-mingw32",
            TargetArch::X86 => "i686-w64-mingw32",
            TargetArch::Aarch64 => "aarch64-w64-mingw32",
            TargetArch::Armv7 => "armv7-w64-mingw32",
            TargetArch::Arm64Ec => "arm64ec-w64-mingw32",
        }
    }

    pub fn msvc_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-pc-windows-msvc",
            TargetArch::X86 => "i686-pc-windows-msvc",
            TargetArch::Aarch64 => "aarch64-pc-windows-msvc",
            TargetArch::Armv7 => "thumbv7a-pc-windows-msvc",
            TargetArch::Arm64Ec => "arm64ec-pc-windows-msvc",
        }
    }

    pub fn msvc_lib_dirs(self) -> &'static [&'static str] {
        match self {
            TargetArch::X86_64 => &["x64", "x86_64"],
            TargetArch::X86 => &["x86"],
            TargetArch::Aarch64 | TargetArch::Arm64Ec => &["arm64", "aarch64"],
            TargetArch::Armv7 => &["arm", "armv7"],
        }
    }

    pub fn rust_gnu_triple(self) -> Option<&'static str> {
        match self {
            TargetArch::X86_64 => Some("x86_64-pc-windows-gnu"),
            TargetArch::X86 => Some("i686-pc-windows-gnu"),
            TargetArch::Aarch64 | TargetArch::Armv7 | TargetArch::Arm64Ec => None,
        }
    }
}

impl std::fmt::Display for TargetArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TargetArch::X86_64 => "x86_64",
            TargetArch::X86 => "x86",
            TargetArch::Aarch64 => "aarch64",
            TargetArch::Armv7 => "armv7",
            TargetArch::Arm64Ec => "arm64ec",
        })
    }
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    #[default]
    Static,
    Dynamic,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemSection {
    #[serde(default = "default_project_drive")]
    pub project_drive: String,
    #[serde(default = "default_mount_project")]
    pub mount_project: String,
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
        read_toml(path)
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
        self.environments().join(name)
    }

    pub fn sysroots(&self) -> PathBuf {
        self.data.join("sysroots")
    }

    pub fn sysroot(&self, name: &str) -> PathBuf {
        self.sysroots().join(name)
    }

    pub fn user_config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }
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

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| ConfigError::read(path, e))?;
    toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

fn write_toml<T: Serialize>(path: &Path, value: &T, what: &'static str) -> Result<()> {
    let text = toml::to_string_pretty(value).map_err(|source| ConfigError::Serialize {
        what,
        source: Box::new(source),
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ConfigError::write(path, e))?;
    }
    fs::write(path, text).map_err(|e| ConfigError::write(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip_preserves_all_fields() {
        let mut m = ProjectManifest::new("hello-win32");
        m.target.api = Some("win10".into());
        m.toolchain.provider = Some("llvm-mingw".into());
        m.toolchain.link = LinkMode::Dynamic;
        m.environment.name = Some("win11-x64".into());
        m.build = Some(CommandSection {
            command: vec!["cmake".into(), "--build".into(), "build".into()],
        });

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_MANIFEST);
        m.save(&path).unwrap();
        let loaded = ProjectManifest::load(&path).unwrap();
        assert_eq!(m, loaded);
        assert_eq!(loaded.toolchain.link, LinkMode::Dynamic);
    }

    #[test]
    fn minimal_manifest_parses_with_defaults() {
        let m: ProjectManifest = toml::from_str("[project]\nname = \"x\"\n").unwrap();
        assert_eq!(m.target.os, "windows");
        assert_eq!(m.target.arch, TargetArch::X86_64);
        assert_eq!(m.runtime.provider, "wine");
        assert_eq!(m.filesystem.project_drive, "C:");
        assert_eq!(m.filesystem.mount_project, "/src");
        assert_eq!(m.toolchain.link, LinkMode::Static);
        assert!(m.build.is_none());
    }

    #[test]
    fn env_section_roundtrips_vars_and_secrets() {
        let src = "[project]\nname = \"x\"\n[env.vars]\nRUST_LOG = \"debug\"\n[env.secret]\nAPI_TOKEN = \"HOST_API_TOKEN\"\n";
        let m: ProjectManifest = toml::from_str(src).unwrap();
        assert_eq!(m.env.vars.get("RUST_LOG").unwrap(), "debug");
        assert_eq!(m.env.secret.get("API_TOKEN").unwrap(), "HOST_API_TOKEN");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_MANIFEST);
        m.save(&path).unwrap();
        assert_eq!(ProjectManifest::load(&path).unwrap(), m);
    }

    #[test]
    fn link_mode_parses_lowercase() {
        let m: ProjectManifest =
            toml::from_str("[project]\nname = \"x\"\n[toolchain]\nlink = \"dynamic\"\n").unwrap();
        assert_eq!(m.toolchain.link, LinkMode::Dynamic);
    }

    #[test]
    fn unknown_manifest_keys_are_rejected() {
        let err = toml::from_str::<ProjectManifest>("[project]\nname = \"x\"\nbogus = 1\n");
        assert!(err.is_err());
    }

    #[test]
    fn discover_walks_upward() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ProjectManifest::new("demo")
            .save(&root.join(PROJECT_MANIFEST))
            .unwrap();
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let (found_root, m) = ProjectManifest::discover(&nested).unwrap();
        assert_eq!(found_root, root);
        assert_eq!(m.project.name, "demo");
    }

    #[test]
    fn discover_fails_outside_projects() {
        let dir = tempfile::tempdir().unwrap();
        let err = ProjectManifest::discover(dir.path()).unwrap_err();
        assert!(err.to_string().contains("LSW1005"));
    }

    #[test]
    fn lockfile_roundtrip() {
        let lock = Lockfile {
            version: 1,
            environment_format: ENVIRONMENT_FORMAT_VERSION,
            target_arch: TargetArch::X86_64,
            toolchain: LockedComponent {
                provider: "llvm-mingw".into(),
                version: "22.1.6".into(),
                sha256: "ab".repeat(32),
            },
            runtime: LockedComponent {
                provider: "wine".into(),
                version: "11.12".into(),
                sha256: "cd".repeat(32),
            },
            sysroot: LockedComponent {
                provider: "mingw-w64".into(),
                version: "unknown".into(),
                sha256: "ef".repeat(32),
            },
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_LOCKFILE);
        lock.save(&path).unwrap();
        assert_eq!(Lockfile::load(&path).unwrap(), lock);
    }

    #[test]
    fn environment_layout_paths() {
        let l = EnvironmentLayout::new(PathBuf::from("/data/lsw/environments/e1"));
        assert_eq!(
            l.prefix(),
            PathBuf::from("/data/lsw/environments/e1/prefix")
        );
        assert_eq!(
            l.drive_c(),
            PathBuf::from("/data/lsw/environments/e1/prefix/drive_c")
        );
        assert_eq!(l.src().file_name().unwrap(), "src");
        assert_eq!(l.temp().file_name().unwrap(), "Temp");
    }

    #[test]
    fn arch_triples() {
        assert_eq!(TargetArch::X86_64.mingw_triple(), "x86_64-w64-mingw32");
        assert_eq!(TargetArch::X86.mingw_triple(), "i686-w64-mingw32");
    }
}
