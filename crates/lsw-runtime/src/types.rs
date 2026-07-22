use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(
        "LSW1501: wine executable not found on PATH; \
         install wine via your package manager (e.g. 'pacman -S wine' or 'apt install wine')"
    )]
    WineNotFound,

    #[error(
        "LSW1502: wine prefix initialization failed: {detail}; \
         delete the prefix directory and re-run, or run 'wineboot -u' manually with WINEPREFIX set to inspect the failure"
    )]
    PrefixInitFailed { detail: String },

    #[error(
        "LSW1503: cannot spawn {}: {source}; \
         check that the file exists and the runtime is installed correctly", program.display()
    )]
    SpawnFailed {
        program: PathBuf,
        source: std::io::Error,
    },

    #[error(
        "LSW1505: strict sandbox requested but bubblewrap (bwrap) is not installed; \
         install bubblewrap or drop --sandbox"
    )]
    SandboxUnavailable,

    #[error(
        "LSW1506: a virtual display was requested but xvfb-run is not installed; \
         install xvfb (the 'xorg-server-xvfb' or 'xvfb' package) or run with a real $DISPLAY"
    )]
    VirtualDisplayUnavailable,

    #[error(
        "LSW1504: runtime execution failed: {detail}; \
         re-run with WINEDEBUG unset (pass it in the request env) for more diagnostics"
    )]
    ExecutionFailed { detail: String },

    #[error("LSW1507: process {pid} is not running in this environment")]
    ProcessNotInEnvironment { pid: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub prefix: PathBuf,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub sandbox: Option<SandboxSpec>,
    pub display: DisplayMode,
    pub emulate: Option<Emulation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emulation {
    pub qemu: PathBuf,
    pub wine: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    #[default]
    Inherit,
    Virtual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkMode {
    Host,
    Isolated,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SandboxSpec {
    pub rw_binds: Vec<PathBuf>,
    pub network: NetworkMode,
    pub cpu_seconds: Option<u64>,
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDiagnostics {
    pub id: String,
    pub version: Option<String>,
    pub executable: Option<PathBuf>,
    pub prefix_exists: bool,
    pub prefix_initialized: bool,
}
