use std::path::PathBuf;

/// Errors from Wine runtime operations.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(
        "LSW1501: wine executable not found on PATH; \
         install wine via your package manager (e.g. 'pacman -S wine' or 'apt install wine'), \
         or point LSW_WINE at a wine binary"
    )]
    /// Wine executable not found on PATH.
    WineNotFound,

    #[error(
        "LSW1508: LSW_WINE points at '{}' which is not an executable file; \
         fix or unset LSW_WINE", path.display()
    )]
    /// `LSW_WINE` environment variable points at a non-executable file.
    WineOverrideInvalid {
        /// Path from the `LSW_WINE` variable.
        path: PathBuf,
    },

    #[error(
        "LSW1502: wine prefix initialization failed: {detail}; \
         delete the prefix directory and re-run, or run 'wineboot -u' manually with WINEPREFIX set to inspect the failure"
    )]
    /// Wine prefix (WINEPREFIX) initialization failed.
    PrefixInitFailed {
        /// Failure detail.
        detail: String,
    },

    #[error(
        "LSW1503: cannot spawn {}: {source}; \
         check that the file exists and the runtime is installed correctly", program.display()
    )]
    /// Failed to spawn a process.
    SpawnFailed {
        /// Program that failed to spawn.
        program: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    #[error(
        "LSW1505: strict sandbox requested but bubblewrap (bwrap) is not installed; \
         install bubblewrap or drop --sandbox"
    )]
    /// Bubblewrap sandbox is not available.
    SandboxUnavailable,

    #[error(
        "LSW1506: a virtual display was requested but xvfb-run is not installed; \
         install xvfb (the 'xorg-server-xvfb' or 'xvfb' package) or run with a real $DISPLAY"
    )]
    /// Xvfb virtual display is not available.
    VirtualDisplayUnavailable,

    #[error(
        "LSW1504: runtime execution failed: {detail}; \
         re-run with WINEDEBUG unset (pass it in the request env) for more diagnostics"
    )]
    /// Wine process execution failed.
    ExecutionFailed {
        /// Failure detail.
        detail: String,
    },

    #[error("LSW1507: process {pid} is not running in this environment")]
    /// Target process does not belong to this environment.
    ProcessNotInEnvironment {
        /// Process ID.
        pid: u32,
    },
}

/// Parameters for executing a program under Wine.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionRequest {
    /// Path to the Windows executable.
    pub program: PathBuf,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Wine prefix directory.
    pub prefix: PathBuf,
    /// Working directory for the process.
    pub cwd: Option<PathBuf>,
    /// Additional environment variables.
    pub env: Vec<(String, String)>,
    /// Optional sandbox configuration.
    pub sandbox: Option<SandboxSpec>,
    /// Display mode (inherit or virtual).
    pub display: DisplayMode,
    /// Cross-architecture emulation via QEMU.
    pub emulate: Option<Emulation>,
}

/// Cross-architecture emulation configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emulation {
    /// Path to the QEMU user-mode emulator.
    pub qemu: PathBuf,
    /// Path to the architecture-matched Wine binary.
    pub wine: PathBuf,
}

/// How to provide a display server to the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// Use the existing $DISPLAY.
    #[default]
    Inherit,
    /// Launch a virtual X server via xvfb-run.
    Virtual,
}

/// Network isolation mode for the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkMode {
    /// Full host network access.
    Host,
    /// Isolated network namespace.
    Isolated,
    /// No network access.
    #[default]
    None,
}

/// Sandbox resource limits and configuration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SandboxSpec {
    /// Directories mounted read-write inside the sandbox.
    pub rw_binds: Vec<PathBuf>,
    /// Network isolation mode.
    pub network: NetworkMode,
    /// CPU time limit in seconds.
    pub cpu_seconds: Option<u64>,
    /// Address-space limit in bytes.
    pub memory_bytes: Option<u64>,
}

/// Runtime diagnostic information.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDiagnostics {
    /// Runtime provider identifier.
    pub id: String,
    /// Runtime version string, if detected.
    pub version: Option<String>,
    /// Path to the runtime executable, if found.
    pub executable: Option<PathBuf>,
    /// Whether the Wine prefix directory exists.
    pub prefix_exists: bool,
    /// Whether the Wine prefix is fully initialized.
    pub prefix_initialized: bool,
}
