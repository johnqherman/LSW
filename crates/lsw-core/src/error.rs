use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
/// Error.
pub enum Error {
    #[error(transparent)]
    /// Config.
    Config(#[from] lsw_config::ConfigError),
    #[error(transparent)]
    /// Path.
    Path(#[from] lsw_path::PathError),
    #[error(transparent)]
    /// Pe.
    Pe(#[from] lsw_pe::PeError),
    #[error(transparent)]
    /// Toolchain.
    Toolchain(#[from] lsw_toolchain::ToolchainError),
    #[error(transparent)]
    /// Runtime.
    Runtime(#[from] lsw_runtime::RuntimeError),

    #[error(
        "LSW2001: no active environment for this project\n\
         Possible fixes:\n  lsw env create <name>\n  lsw use <name>"
    )]
    /// No Active Environment.
    NoActiveEnvironment,

    #[error(
        "LSW2002: environment '{name}' does not exist\n\
         Possible fixes:\n  lsw env create {name}\n  lsw env list"
    )]
    /// Environment Not Found.
    EnvironmentNotFound {
        /// Name.
        name: String,
    },

    #[error(
        "LSW2003: environment '{name}' already exists\n\
         Possible fixes:\n  lsw env remove {name}\n  choose another name"
    )]
    /// Environment Exists.
    EnvironmentExists {
        /// Name.
        name: String,
    },

    #[error("LSW2004: '{}' is not something LSW can execute: {detail}", program.display())]
    /// Not Executable.
    NotExecutable {
        /// Program.
        program: PathBuf,
        /// Detail.
        detail: String,
    },

    #[error(
        "LSW2005: build failed (exit code {code:?})\n\
         Command: {command}"
    )]
    /// Build Failed.
    BuildFailed {
        /// Command.
        command: String,
        /// Code.
        code: Option<i32>,
    },

    #[error(
        "LSW2006: lsw.lock does not match environment '{environment}'\n\
         {detail}\n\
         Possible fixes:\n  lsw build --update-lock\n  lsw env remove {environment} && lsw env create {environment}"
    )]
    /// Lock Mismatch.
    LockMismatch {
        /// Environment.
        environment: String,
        /// Detail.
        detail: String,
    },

    #[error(
        "LSW2007: no build system detected\n\
         Expected CMakeLists.txt, meson.build, build.zig, Cargo.toml, a .csproj/.sln, \
         build.ninja, a Makefile, or a [build] command in lsw.toml"
    )]
    /// No Build System.
    NoBuildSystem,

    #[error("LSW2008: target os '{os}' is not supported (only 'windows')")]
    /// Unsupported Target Os.
    UnsupportedTargetOs {
        /// Os.
        os: String,
    },

    #[error("LSW2009: cannot create project at {}: {detail}", path.display())]
    /// Init Failed.
    InitFailed {
        /// Path.
        path: PathBuf,
        /// Detail.
        detail: String,
    },

    #[error("LSW2010: io error at {}: {source}", path.display())]
    /// Io.
    Io {
        /// Path.
        path: PathBuf,
        /// Source.
        source: std::io::Error,
    },

    #[error("LSW2011: required tool '{tool}' not found on PATH\nPossible fixes: {fix}")]
    /// Tool Missing.
    ToolMissing {
        /// Tool.
        tool: String,
        /// Fix.
        fix: String,
    },

    #[error(
        "LSW2012: invalid {kind} name '{name}'\n\
         Names must be non-empty and must not contain path separators, '..', or NUL"
    )]
    /// Invalid Name.
    InvalidName {
        /// Kind.
        kind: String,
        /// Name.
        name: String,
    },

    #[error(
        "LSW2016: process {pid} does not belong to environment '{environment}' (or already exited)\n\
         Use 'lsw ps' to list this environment's processes"
    )]
    /// Process Not In Environment.
    ProcessNotInEnvironment {
        /// Pid.
        pid: u32,
        /// Environment.
        environment: String,
    },

    #[error(
        "LSW2015: registry operation failed (exit code {code:?})\n\
         Check the key path (e.g. 'HKCU\\Software\\Example\\App') and see the output above"
    )]
    /// Registry Operation Failed.
    RegistryOperationFailed {
        /// Code.
        code: Option<i32>,
    },

    #[error(
        "LSW2014: nothing to test\n\
         Possible fixes:\n  \
         add add_test(...) to CMakeLists.txt and rebuild, or\n  \
         set [test].command in lsw.toml"
    )]
    /// No Tests.
    NoTests,

    #[error("LSW2022: provider plugin '{name}' protocol error: {detail}")]
    /// Plugin Protocol.
    PluginProtocol {
        /// Name.
        name: String,
        /// Detail.
        detail: String,
    },

    #[error("LSW2026: service '{op}' failed for '{name}': {detail}")]
    /// Service Failed.
    ServiceFailed {
        /// Op.
        op: String,
        /// Name.
        name: String,
        /// Detail.
        detail: String,
    },

    #[error("LSW2027: compatibility database error: {detail}")]
    /// Compat Db.
    CompatDb {
        /// Detail.
        detail: String,
    },

    #[error("LSW2028: debug adapter protocol error: {detail}")]
    /// Dap.
    Dap {
        /// Detail.
        detail: String,
    },

    #[error("LSW2029: MSIX signing failed: {detail}")]
    /// Msix Sign.
    MsixSign {
        /// Detail.
        detail: String,
    },

    #[error("LSW2030: invalid [sandbox] network = \"{value}\" (expected host, isolated, or none)")]
    /// Invalid Sandbox Network.
    InvalidSandboxNetwork {
        /// Value.
        value: String,
    },

    #[error(
        "LSW2025: Rust has no GNU-ABI Windows target for arch '{arch}'\n\
         Rust builds support x86_64, x86, and aarch64. armv7/arm64ec are MSVC-only in Rust."
    )]
    /// Rust Target Unavailable.
    RustTargetUnavailable {
        /// Arch.
        arch: String,
    },

    #[error(
        "LSW2024: unsafe value '{value}' for native verification\n\
         Remote paths and artifact names must be a drive-letter path with segments of [A-Za-z0-9._+-] only.\n\
         This prevents command injection on the Windows host."
    )]
    /// Unsafe Remote Path.
    UnsafeRemotePath {
        /// Value.
        value: String,
    },

    #[error(
        "LSW2021: unsupported verification transport '{transport}'\n\
         Supported transports: 'ssh', 'winrm', 'https'; set one in [verify]"
    )]
    /// Unsupported Transport.
    UnsupportedTransport {
        /// Transport.
        transport: String,
    },

    #[error(
        "LSW2045: refusing to bind '{}' into a strict sandbox\n\
         The project or environment resolves to a system directory; strict isolation would grant the guest writable host access. Move the project out of the filesystem root or a system path.",
        path.display()
    )]
    /// Unsafe Sandbox Bind.
    UnsafeSandboxBind {
        /// Path.
        path: std::path::PathBuf,
    },

    #[error(
        "LSW2019: SDK '{name}' already exists\n\
         Possible fixes:\n  lsw sdk import {name} --from <path> --force\n  lsw sdk remove {name}"
    )]
    /// Sdk Exists.
    SdkExists {
        /// Name.
        name: String,
    },

    #[error(
        "LSW2020: SDK '{name}' does not exist\n\
         List imported SDKs with: lsw sdk list"
    )]
    /// Sdk Not Found.
    SdkNotFound {
        /// Name.
        name: String,
    },

    #[error(
        "LSW2018: two build artifacts share the name '{name}' ({} and {})\n\
         Packaging them flat would ship the wrong binary. Rename a target or build a single configuration.",
        first.display(), second.display()
    )]
    /// Package Name Collision.
    PackageNameCollision {
        /// Name.
        name: String,
        /// First.
        first: PathBuf,
        /// Second.
        second: PathBuf,
    },

    #[error(
        "LSW2017: the build was not configured to run Windows tests through the runtime\n\
         Test binaries would execute as host processes and a pass would be meaningless.\n\
         Possible fix: remove the build/ directory and re-run `lsw test` (a fresh configure sets the emulator)"
    )]
    /// Test Emulator Missing.
    TestEmulatorMissing,

    #[error(
        "LSW2013: build produced '{}' which is not a Windows PE binary ({found})\n\
         The build ran with host tools but did not cross-compile.\n\
         Possible fixes:\n  \
         use the generated CMake toolchain (default `lsw build`), or\n  \
         make your [build] command honor CC/CXX/CFLAGS/CXXFLAGS/LDFLAGS", artifact.display()
    )]
    /// Artifact Not Pe.
    ArtifactNotPe {
        /// Artifact.
        artifact: PathBuf,
        /// Found.
        found: String,
    },

    #[error("LSW2031: cannot read crash dump {}: {detail}", path.display())]
    /// Dump Parse.
    DumpParse {
        /// Path.
        path: PathBuf,
        /// Detail.
        detail: String,
    },

    #[error("LSW2032: native import probe failed on '{host}': {detail}")]
    /// Probe Failed.
    ProbeFailed {
        /// Host.
        host: String,
        /// Detail.
        detail: String,
    },

    #[error(
        "LSW2033: package '{name}' not found in the {repo} package set\n\
         Names follow the upstream library (e.g. zlib, sqlite3, libpng)."
    )]
    /// Dep Not Found.
    DepNotFound {
        /// Name.
        name: String,
        /// Repo.
        repo: String,
    },

    #[error("LSW2034: could not fetch {url}: {detail}")]
    /// Download Failed.
    DownloadFailed {
        /// Url.
        url: String,
        /// Detail.
        detail: String,
    },

    #[error("LSW2035: checksum mismatch for {name} (expected {expected}, got {actual})")]
    /// Checksum Mismatch.
    ChecksumMismatch {
        /// Name.
        name: String,
        /// Expected.
        expected: String,
        /// Actual.
        actual: String,
    },

    #[error("LSW2036: could not unpack {name}: {detail}")]
    /// Extract Failed.
    ExtractFailed {
        /// Name.
        name: String,
        /// Detail.
        detail: String,
    },

    #[error("LSW2037: {arch} has no mingw-w64 package repository")]
    /// Dep Arch Unsupported.
    DepArchUnsupported {
        /// Arch.
        arch: String,
    },

    #[error(
        "LSW2038: cross-architecture execution needs a {arch} Wine build\n\
         Set {var} to a {arch} wine executable (run under qemu user-mode emulation).\n\
         Set QEMU_LD_PREFIX to that Wine's sysroot if its libraries are not on the default path."
    )]
    /// Emulation Wine Missing.
    EmulationWineMissing {
        /// Arch.
        arch: String,
        /// Var.
        var: String,
    },

    #[error(
        "LSW2039: [filesystem] case = \"strict\" but the project has case-insensitive name collisions\n\
         {detail}\n\
         These files coexist on Linux but clash on Windows. Rename them, or use case = \"native\"."
    )]
    /// Case Collision.
    CaseCollision {
        /// Detail.
        detail: String,
    },

    #[error("LSW2040: installer verification failed during {stage}: {detail}")]
    /// Install Verify Failed.
    InstallVerifyFailed {
        /// Stage.
        stage: String,
        /// Detail.
        detail: String,
    },

    #[error("LSW2041: NativeAOT cross-compilation is unavailable: {detail}")]
    /// Aot Unsupported.
    AotUnsupported {
        /// Detail.
        detail: String,
    },

    #[error("LSW2042: SDK import from {} failed: {detail}", path.display())]
    /// Sdk Import Failed.
    SdkImportFailed {
        /// Path.
        path: PathBuf,
        /// Detail.
        detail: String,
    },

    #[error(
        "LSW2044: cannot restore the SDK/MSVC toolchain '{provider}' from lsw.lock (the lockfile records no SDK identity)\n\
         Recreate it with: lsw sdk import <name> --from <path>  then  lsw env create {name} --sdk <name>"
    )]
    /// Restore Unsupported Toolchain.
    RestoreUnsupportedToolchain {
        /// Provider.
        provider: String,
        /// Name.
        name: String,
    },

    #[error(
        "LSW2046: unknown build system '{name}'\n\
         Valid values: cmake, cargo, make, ninja, meson, zig, dotnet, or explicit (needs a [build] command in lsw.toml)"
    )]
    /// Unknown Build System.
    UnknownBuildSystem {
        /// Name.
        name: String,
    },
}

impl Error {
    /// Io.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    /// Code.
    pub fn code(&self) -> String {
        let text = self.to_string();
        text.split(':')
            .next()
            .filter(|head| head.starts_with("LSW"))
            .unwrap_or("LSW0000")
            .to_owned()
    }
}

/// Result type alias.
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
        let e = Error::EnvironmentNotFound { name: "dev".into() };
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
