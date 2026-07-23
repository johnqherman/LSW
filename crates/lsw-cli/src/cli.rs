use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use lsw_core::{Domain, TargetArch};

#[derive(Parser)]
#[command(
    name = "lsw",
    version,
    about = "Linux Subsystem for Windows Development",
    long_about = "Build, run, and inspect Windows applications without leaving Linux."
)]
pub(crate) struct Cli {
    /// Verbose diagnostic logging.
    #[arg(long, global = true)]
    pub(crate) verbose: bool,
    /// Maximum-detail trace logging (implies --verbose).
    #[arg(long, global = true)]
    pub(crate) trace: bool,
    /// Output format for machine consumption where supported.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    pub(crate) format: Format,
    #[command(subcommand)]
    pub(crate) command: Cmd,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    Human,
    Json,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// Scaffold a new project (lsw.toml + CMake template).
    Init {
        /// Project name; omit to initialize the current directory.
        name: Option<String>,
        /// Project template.
        #[arg(long, value_enum, default_value_t = TemplateArg::Console)]
        template: TemplateArg,
    },
    /// Manage isolated Windows-target environments.
    #[command(subcommand)]
    Env(EnvCmd),
    /// Select the active environment for this project.
    Use { name: String },
    /// Build Windows artifacts using native Linux tools.
    Build {
        /// Force a build system (cmake, cargo, make, ninja, meson, zig, dotnet).
        #[arg(long)]
        system: Option<String>,
        /// Refresh lsw.lock instead of failing on drift.
        #[arg(long)]
        update_lock: bool,
        /// Zero PE timestamps for reproducible, byte-identical artifacts.
        #[arg(long)]
        reproducible: bool,
        /// Compile C# with NativeAOT to a native PE (dotnet projects only).
        #[arg(long)]
        aot: bool,
    },
    /// Run an executable (PE via the Windows runtime, ELF natively).
    Run {
        /// Program to run; omit to build and run the project's single executable.
        program: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Force the host (Linux) execution domain.
        #[arg(long, conflicts_with = "windows")]
        host: bool,
        /// Force the Windows execution domain.
        #[arg(long)]
        windows: bool,
        /// Kernel sandbox for Windows execution ("strict").
        #[arg(long)]
        sandbox: Option<SandboxArg>,
        /// Run GUI programs under a virtual display (headless CI).
        #[arg(long)]
        headless: bool,
    },
    /// Run a command in an explicit execution domain.
    Exec {
        /// Host (Linux) domain.
        #[arg(long, conflicts_with = "windows")]
        host: bool,
        /// Windows domain.
        #[arg(long)]
        windows: bool,
        /// Kernel sandbox for Windows execution ("strict").
        #[arg(long)]
        sandbox: Option<SandboxArg>,
        /// Run GUI programs under a virtual display (headless CI).
        #[arg(long)]
        headless: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Build, then run tests under the local compatibility runtime.
    Test {
        /// CI mode: no interactive display (exports LSW_HEADLESS=1).
        #[arg(long)]
        headless: bool,
    },
    /// Build, then verify artifacts on a real Windows host.
    Verify {
        /// Run on native Windows (the only supported mode today).
        #[arg(long)]
        native_windows: bool,
    },
    /// Interactive shell: Linux with Windows-target env, or cmd.exe.
    Shell {
        /// Launch an actual Windows shell (cmd.exe) in the environment.
        #[arg(long)]
        windows: bool,
    },
    /// Inspect a PE binary: format, architecture, subsystem, imports.
    Inspect { file: PathBuf },
    /// Decode a Windows crash dump: exception, faulting module, address.
    Crash { file: PathBuf },
    /// Audit a PE's security hardening (ASLR, DEP, CFG, SafeSEH, signing).
    Audit { file: PathBuf },
    /// List the exported symbols of a PE (mirror of imports).
    Exports { file: PathBuf },
    /// Generate a CycloneDX SBOM for a PE (imports + toolchain provenance).
    Sbom { file: PathBuf },
    /// Diff two PEs by imports and exports.
    Diff { a: PathBuf, b: PathBuf },
    /// Extract printable ASCII and UTF-16 strings from a file.
    Strings {
        file: PathBuf,
        /// Minimum string length.
        #[arg(long, default_value_t = 4)]
        min: usize,
    },
    /// Inspect a PE's dependencies.
    #[command(subcommand)]
    Deps(DepsCmd),
    /// Generate CI configuration.
    #[command(subcommand)]
    Ci(CiCmd),
    /// Validate lsw.toml settings.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Authenticode-sign a PE with a cached self-signed identity.
    Sign {
        file: PathBuf,
        /// Certificate subject (default: a self-signed LSW identity).
        #[arg(long)]
        publisher: Option<String>,
    },
    /// Translate paths between Linux and Windows views.
    Path {
        /// Print the Windows form of a Linux path.
        #[arg(long, conflicts_with = "linux")]
        windows: Option<PathBuf>,
        /// Print the Linux form of a Windows path.
        #[arg(long)]
        linux: Option<String>,
    },
    /// Read and write the environment's isolated registry.
    #[command(subcommand, alias = "reg")]
    Registry(RegistryCmd),
    /// Debug a Windows binary with winedbg (or its gdb proxy).
    Debug {
        program: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Start a gdbserver-compatible proxy instead of the console.
        #[arg(long)]
        gdb: bool,
        /// With --gdb: print connection info only, don't launch host gdb.
        #[arg(long, requires = "gdb")]
        no_start: bool,
        /// Run on the [verify] Windows host under cdb for a real backtrace.
        #[arg(long, conflicts_with_all = ["gdb", "no_start"])]
        native: bool,
    },
    /// Run a Debug Adapter Protocol server over stdio for IDEs.
    Dap,
    /// Measured compatibility report (imports + runtime trace).
    Compat {
        program: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Record observations into the persistent compatibility database.
        #[arg(long)]
        db: bool,
        /// Also run the binary on the configured [verify] Windows host for a native verdict.
        #[arg(long)]
        native: bool,
    },
    /// Query the persistent compatibility database.
    CompatQuery {
        /// A DLL name or `module!function` to look up.
        key: String,
    },
    /// Trace a Windows binary's DLL loads and API calls under the runtime.
    Trace {
        program: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Also capture the full (very verbose) relay call trace.
        #[arg(long)]
        relay: bool,
    },
    /// Build and assemble a distributable package.
    Package {
        /// Package target.
        #[arg(long, value_enum, default_value_t = PackageTargetArg::Zip)]
        target: PackageTargetArg,
        /// Install-test the MSI in a scratch environment (msi target only).
        #[arg(long)]
        verify: bool,
    },
    /// List the environment's Windows/runtime processes.
    Ps {
        /// Include wine infrastructure processes (wineserver, services.exe, ...).
        #[arg(long)]
        all: bool,
    },
    /// Terminate an environment process (or all with --all).
    Kill {
        pid: Option<u32>,
        /// Shut down every process in the environment.
        #[arg(long, conflicts_with = "pid")]
        all: bool,
    },
    /// Manage Windows services inside the environment.
    #[command(subcommand)]
    Service(ServiceCmd),
    /// First-class Rust support.
    #[command(subcommand)]
    Rust(RustCmd),
    /// C#/.NET support (console, self-contained; managed, not native).
    #[command(subcommand)]
    Dotnet(DotnetCmd),
    /// Talk to the optional lswd daemon.
    #[command(subcommand)]
    Daemon(DaemonCmd),
    /// Discover and inspect out-of-process provider plugins.
    #[command(subcommand)]
    Plugin(PluginCmd),
    /// Import and manage user-provided Windows SDK sysroots.
    #[command(subcommand)]
    Sdk(SdkCmd),
    /// IDE integration helpers.
    #[command(subcommand)]
    Ide(IdeCmd),
    /// Rebuild automatically when project source files change.
    Watch,
    /// Diagnose host, runtime, toolchain, and project health.
    Doctor,
    /// Generate shell completion scripts (bash, zsh, fish, powershell, elvish).
    Completions { shell: clap_complete::Shell },
    /// Generate man pages (top-level to stdout, or all pages into --dir).
    Man {
        /// Directory to write lsw.1 and lsw-<subcommand>.1 into.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Install shell completions and man pages (the binary comes from cargo/your package manager).
    Install {
        /// Install prefix (default: $PREFIX, else ~/.local).
        #[arg(long)]
        prefix: Option<PathBuf>,
    },
    /// Explain an LSW#### error code.
    Explain { code: String },
}

#[derive(Subcommand)]
pub(crate) enum DepsCmd {
    /// Print the transitive DLL dependency tree.
    Tree { file: PathBuf },
    /// Install a mingw-w64 library (headers, import/static libs, DLLs).
    Add { name: String },
    /// Remove an installed library.
    Remove { name: String },
    /// List installed libraries.
    List,
}

#[derive(Subcommand)]
pub(crate) enum CiCmd {
    /// Write a GitHub Actions workflow (.github/workflows/lsw.yml).
    Init {
        #[arg(value_enum, default_value_t = CiProvider::Github)]
        provider: CiProvider,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum CiProvider {
    Github,
}

#[derive(Subcommand)]
pub(crate) enum ConfigCmd {
    /// Lint the project's lsw.toml for invalid or unrecognized settings.
    Check,
}

#[derive(Subcommand)]
pub(crate) enum IdeCmd {
    /// Print the environment description IDE plugins consume (JSON).
    Env,
}

#[derive(Subcommand)]
pub(crate) enum PluginCmd {
    /// List discovered `lsw-provider-*` plugins and their handshake info.
    List,
}

#[derive(Subcommand)]
pub(crate) enum DaemonCmd {
    /// Show whether the daemon is running and its version.
    Status,
    /// Ask a running daemon to stop.
    Stop,
}

#[derive(Subcommand)]
pub(crate) enum RustCmd {
    /// Scaffold a Rust project wired for Windows targeting.
    Init { name: Option<String> },
    /// Report Rust->Windows toolchain readiness for the active environment.
    Doctor,
}

#[derive(Subcommand)]
pub(crate) enum DotnetCmd {
    /// Scaffold a C# console project wired for Windows targeting.
    Init { name: Option<String> },
    /// Report C#/.NET->Windows toolchain readiness for the active environment.
    Doctor,
}

#[derive(Subcommand)]
pub(crate) enum ServiceCmd {
    /// Register a service from a Windows binary path.
    Create {
        name: String,
        /// Windows path to the service executable (e.g. C:\svc\app.exe).
        #[arg(long)]
        bin: String,
    },
    /// Start a service.
    Start { name: String },
    /// Stop a service.
    Stop { name: String },
    /// Query a service's state.
    Query { name: String },
    /// Delete a service.
    Delete { name: String },
}

#[derive(Subcommand)]
pub(crate) enum SdkCmd {
    /// Import a user-obtained SDK directory as a named sysroot.
    Import {
        name: String,
        /// Path to the SDK content the user has legally obtained.
        #[arg(long)]
        from: PathBuf,
        /// Overwrite an existing SDK of the same name.
        #[arg(long)]
        force: bool,
    },
    /// List imported SDKs.
    List,
    /// Remove an imported SDK.
    Remove { name: String },
}

#[derive(Subcommand)]
pub(crate) enum RegistryCmd {
    /// Query a key (optionally one value), e.g. 'HKCU\Software\Example'.
    Get { key: String, value: Option<String> },
    /// Set a value under a key.
    Set {
        key: String,
        value: String,
        data: String,
        /// Registry value type.
        #[arg(long, default_value = "REG_SZ")]
        kind: String,
    },
    /// Export a key to a .reg file on the host.
    Export { key: String, file: PathBuf },
    /// Merge a host .reg file into the environment's registry.
    Import { file: PathBuf },
    /// Apply the project's [registry] seeds to the active environment.
    Seed,
    /// Discard all registry state and rebuild prefix defaults.
    Reset,
}

#[derive(Subcommand)]
pub(crate) enum EnvCmd {
    /// Create an isolated environment (Wine prefix + toolchain probe).
    Create {
        name: String,
        /// Target architecture.
        #[arg(long, default_value = "x86_64")]
        arch: ArchArg,
        /// Toolchain provider id (llvm-mingw, mingw-gcc); default auto-probe.
        #[arg(long)]
        toolchain: Option<String>,
        /// Build against an imported Windows SDK using the clang-cl MSVC ABI.
        /// Names an SDK from `lsw sdk import`.
        #[arg(long)]
        sdk: Option<String>,
        /// Recreate if it already exists.
        #[arg(long)]
        force: bool,
        /// Keep the host home directory visible to Windows programs (default hides it).
        #[arg(long)]
        expose_home: bool,
    },
    /// List environments.
    List,
    /// Recreate an environment from lsw.lock and verify it matches the pins.
    Restore { name: String },
    /// Clone an environment (reflink copy where the filesystem supports it).
    Clone {
        src: String,
        dst: String,
        /// Overwrite the destination if it exists.
        #[arg(long)]
        force: bool,
    },
    /// Delete an environment and its Wine prefix.
    Remove { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum SandboxArg {
    #[value(name = "strict")]
    Strict,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum TemplateArg {
    Console,
    Gui,
    Dll,
}

impl From<TemplateArg> for lsw_core::Template {
    fn from(value: TemplateArg) -> Self {
        match value {
            TemplateArg::Console => lsw_core::Template::Console,
            TemplateArg::Gui => lsw_core::Template::Gui,
            TemplateArg::Dll => lsw_core::Template::Dll,
        }
    }
}

pub(crate) fn sandbox_from(a: Option<SandboxArg>) -> lsw_core::Sandbox {
    match a {
        Some(SandboxArg::Strict) => lsw_core::Sandbox::Strict,
        None => lsw_core::Sandbox::None,
    }
}

pub(crate) fn display_from(headless: bool) -> lsw_core::Display {
    if headless {
        lsw_core::Display::Headless
    } else {
        lsw_core::Display::Auto
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum PackageTargetArg {
    #[value(name = "portable-directory")]
    PortableDirectory,
    #[value(name = "zip")]
    Zip,
    #[value(name = "msi")]
    Msi,
    #[value(name = "msix")]
    Msix,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum ArchArg {
    #[value(name = "x86_64")]
    X86_64,
    #[value(name = "x86")]
    X86,
    #[value(name = "aarch64")]
    Aarch64,
    #[value(name = "armv7")]
    Armv7,
    #[value(name = "arm64ec")]
    Arm64Ec,
}

impl From<ArchArg> for TargetArch {
    fn from(a: ArchArg) -> Self {
        match a {
            ArchArg::X86_64 => TargetArch::X86_64,
            ArchArg::X86 => TargetArch::X86,
            ArchArg::Aarch64 => TargetArch::Aarch64,
            ArchArg::Armv7 => TargetArch::Armv7,
            ArchArg::Arm64Ec => TargetArch::Arm64Ec,
        }
    }
}

pub(crate) fn domain_from_flags(host: bool, windows: bool) -> Domain {
    if host {
        Domain::Host
    } else if windows {
        Domain::Windows
    } else {
        Domain::Auto
    }
}
