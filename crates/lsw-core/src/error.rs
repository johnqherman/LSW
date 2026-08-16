use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] lsw_config::ConfigError),
    #[error(transparent)]
    Path(#[from] lsw_path::PathError),
    #[error(transparent)]
    Pe(#[from] lsw_pe::PeError),
    #[error(transparent)]
    Toolchain(#[from] lsw_toolchain::ToolchainError),
    #[error(transparent)]
    Runtime(#[from] lsw_runtime::RuntimeError),

    #[error(
        "LSW2001: no active environment for this project\n\
         Possible fixes:\n  lsw env create <name>\n  lsw use <name>"
    )]
    NoActiveEnvironment,

    #[error(
        "LSW2002: environment '{name}' does not exist\n\
         Possible fixes:\n  lsw env create {name}\n  lsw env list"
    )]
    EnvironmentNotFound { name: String },

    #[error(
        "LSW2003: environment '{name}' already exists\n\
         Possible fixes:\n  lsw env remove {name}\n  choose another name"
    )]
    EnvironmentExists { name: String },

    #[error("LSW2004: '{}' is not something LSW can execute: {detail}", program.display())]
    NotExecutable { program: PathBuf, detail: String },

    #[error(
        "LSW2005: build failed (exit code {code:?})\n\
         Command: {command}"
    )]
    BuildFailed { command: String, code: Option<i32> },

    #[error(
        "LSW2006: lsw.lock does not match environment '{environment}'\n\
         {detail}\n\
         Possible fixes:\n  lsw build --update-lock\n  lsw env remove {environment} && lsw env create {environment}"
    )]
    LockMismatch { environment: String, detail: String },

    #[error(
        "LSW2007: no build system detected\n\
         Expected CMakeLists.txt, meson.build, build.zig, Cargo.toml, a .csproj/.sln, \
         build.ninja, a Makefile, or a [build] command in lsw.toml"
    )]
    NoBuildSystem,

    #[error("LSW2008: target os '{os}' is not supported (only 'windows')")]
    UnsupportedTargetOs { os: String },

    #[error("LSW2009: cannot create project at {}: {detail}", path.display())]
    InitFailed { path: PathBuf, detail: String },

    #[error("LSW2010: io error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("LSW2011: required tool '{tool}' not found on PATH\nPossible fixes: {fix}")]
    ToolMissing { tool: String, fix: String },

    #[error(
        "LSW2012: invalid {kind} name '{name}'\n\
         Names must be non-empty and must not contain path separators, '..', or NUL"
    )]
    InvalidName { kind: String, name: String },

    #[error(
        "LSW2016: process {pid} does not belong to environment '{environment}' (or already exited)\n\
         Use 'lsw ps' to list this environment's processes"
    )]
    ProcessNotInEnvironment { pid: u32, environment: String },

    #[error(
        "LSW2015: registry operation failed (exit code {code:?})\n\
         Check the key path (e.g. 'HKCU\\Software\\Example\\App') and see the output above"
    )]
    RegistryOperationFailed { code: Option<i32> },

    #[error(
        "LSW2014: nothing to test\n\
         Possible fixes:\n  \
         add add_test(...) to CMakeLists.txt and rebuild, or\n  \
         set [test].command in lsw.toml"
    )]
    NoTests,

    #[error("LSW2022: provider plugin '{name}' protocol error: {detail}")]
    PluginProtocol { name: String, detail: String },

    #[error("LSW2026: service '{op}' failed for '{name}': {detail}")]
    ServiceFailed {
        op: String,
        name: String,
        detail: String,
    },

    #[error("LSW2027: compatibility database error: {detail}")]
    CompatDb { detail: String },

    #[error("LSW2028: debug adapter protocol error: {detail}")]
    Dap { detail: String },

    #[error("LSW2029: MSIX signing failed: {detail}")]
    MsixSign { detail: String },

    #[error("LSW2030: invalid [sandbox] network = \"{value}\" (expected host, isolated, or none)")]
    InvalidSandboxNetwork { value: String },

    #[error(
        "LSW2025: Rust has no GNU-ABI Windows target for arch '{arch}'\n\
         Rust builds support x86_64, x86, and aarch64. armv7/arm64ec are MSVC-only in Rust."
    )]
    RustTargetUnavailable { arch: String },

    #[error(
        "LSW2023: lsw daemon not available at {}: {detail}\n\
         Start it with: lswd  (the daemon is optional; most commands work without it)",
        path.display()
    )]
    DaemonUnavailable { path: PathBuf, detail: String },

    #[error(
        "LSW2024: unsafe value '{value}' for native verification\n\
         Remote paths and artifact names must be a drive-letter path with segments of [A-Za-z0-9._+-] only.\n\
         This prevents command injection on the Windows host."
    )]
    UnsafeRemotePath { value: String },

    #[error(
        "LSW2021: unsupported verification transport '{transport}'\n\
         Supported transports: 'ssh', 'winrm', 'https'; set one in [verify]"
    )]
    UnsupportedTransport { transport: String },

    #[error(
        "LSW2045: refusing to bind '{}' into a strict sandbox\n\
         The project or environment resolves to a system directory; strict isolation would grant the guest writable host access. Move the project out of the filesystem root or a system path.",
        path.display()
    )]
    UnsafeSandboxBind { path: std::path::PathBuf },

    #[error(
        "LSW2019: SDK '{name}' already exists\n\
         Possible fixes:\n  lsw sdk import {name} --from <path> --force\n  lsw sdk remove {name}"
    )]
    SdkExists { name: String },

    #[error(
        "LSW2020: SDK '{name}' does not exist\n\
         List imported SDKs with: lsw sdk list"
    )]
    SdkNotFound { name: String },

    #[error(
        "LSW2018: two build artifacts share the name '{name}' ({} and {})\n\
         Packaging them flat would ship the wrong binary. Rename a target or build a single configuration.",
        first.display(), second.display()
    )]
    PackageNameCollision {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },

    #[error(
        "LSW2017: the build was not configured to run Windows tests through the runtime\n\
         Test binaries would execute as host processes and a pass would be meaningless.\n\
         Possible fix: remove the build/ directory and re-run `lsw test` (a fresh configure sets the emulator)"
    )]
    TestEmulatorMissing,

    #[error(
        "LSW2013: build produced '{}' which is not a Windows PE binary ({found})\n\
         The build ran with host tools but did not cross-compile.\n\
         Possible fixes:\n  \
         use the generated CMake toolchain (default `lsw build`), or\n  \
         make your [build] command honor CC/CXX/CFLAGS/CXXFLAGS/LDFLAGS", artifact.display()
    )]
    ArtifactNotPe { artifact: PathBuf, found: String },

    #[error("LSW2031: cannot read crash dump {}: {detail}", path.display())]
    DumpParse { path: PathBuf, detail: String },

    #[error("LSW2032: native import probe failed on '{host}': {detail}")]
    ProbeFailed { host: String, detail: String },

    #[error(
        "LSW2033: package '{name}' not found in the {repo} package set\n\
         Names follow the upstream library (e.g. zlib, sqlite3, libpng)."
    )]
    DepNotFound { name: String, repo: String },

    #[error("LSW2034: could not fetch {url}: {detail}")]
    DownloadFailed { url: String, detail: String },

    #[error("LSW2035: checksum mismatch for {name} (expected {expected}, got {actual})")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },

    #[error("LSW2036: could not unpack {name}: {detail}")]
    ExtractFailed { name: String, detail: String },

    #[error("LSW2037: {arch} has no mingw-w64 package repository")]
    DepArchUnsupported { arch: String },

    #[error(
        "LSW2038: cross-architecture execution needs a {arch} Wine build\n\
         Set {var} to a {arch} wine executable (run under qemu user-mode emulation).\n\
         Set QEMU_LD_PREFIX to that Wine's sysroot if its libraries are not on the default path."
    )]
    EmulationWineMissing { arch: String, var: String },

    #[error(
        "LSW2039: [filesystem] case = \"strict\" but the project has case-insensitive name collisions\n\
         {detail}\n\
         These files coexist on Linux but clash on Windows. Rename them, or use case = \"native\"."
    )]
    CaseCollision { detail: String },

    #[error("LSW2040: installer verification failed during {stage}: {detail}")]
    InstallVerifyFailed { stage: String, detail: String },

    #[error("LSW2041: NativeAOT cross-compilation is unavailable: {detail}")]
    AotUnsupported { detail: String },

    #[error("LSW2042: SDK import from {} failed: {detail}", path.display())]
    SdkImportFailed { path: PathBuf, detail: String },

    #[error(
        "LSW2044: cannot restore the SDK/MSVC toolchain '{provider}' from lsw.lock (the lockfile records no SDK identity)\n\
         Recreate it with: lsw sdk import <name> --from <path>  then  lsw env create {name} --sdk <name>"
    )]
    RestoreUnsupportedToolchain { provider: String, name: String },

    #[error(
        "LSW2046: unknown build system '{name}'\n\
         Valid values: cmake, cargo, make, ninja, meson, zig, dotnet, or explicit (needs a [build] command in lsw.toml)"
    )]
    UnknownBuildSystem { name: String },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    pub fn code(&self) -> String {
        let text = self.to_string();
        text.split(':')
            .next()
            .filter(|head| head.starts_with("LSW"))
            .unwrap_or("LSW0000")
            .to_owned()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_extracts_lsw_prefix() {
        let e = Error::NoActiveEnvironment;
        assert_eq!(e.code(), "LSW2001");
    }

    #[test]
    fn code_extracts_from_transparent_variants() {
        let inner = lsw_pe::PeError::NotPe {
            path: PathBuf::from("x.txt"),
        };
        let e = Error::Pe(inner);
        assert!(e.code().starts_with("LSW"));
    }

    #[test]
    fn io_helper_builds_variant() {
        let src = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let e = Error::io("/tmp/x", src);
        assert!(e.to_string().contains("LSW2010"));
        assert!(e.to_string().contains("/tmp/x"));
    }

    #[test]
    fn display_includes_error_code_and_detail() {
        let e = Error::EnvironmentNotFound {
            name: "dev".into(),
        };
        let msg = e.to_string();
        assert!(msg.contains("LSW2002"));
        assert!(msg.contains("dev"));
    }

    #[test]
    fn build_failed_shows_command() {
        let e = Error::BuildFailed {
            command: "cmake --build .".into(),
            code: Some(2),
        };
        assert!(e.to_string().contains("cmake --build ."));
    }

    #[test]
    fn all_named_variants_have_lsw_codes() {
        let samples: Vec<Error> = vec![
            Error::NoActiveEnvironment,
            Error::NoBuildSystem,
            Error::NoTests,
            Error::TestEmulatorMissing,
        ];
        for e in samples {
            let code = e.code();
            assert!(code.starts_with("LSW2"), "missing code in: {e}");
        }
    }
}
