use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use lsw_core::{BuildOptions, Dirs, Domain, EnvCreateOptions, Project, Status, TargetArch};

#[derive(Parser)]
#[command(
    name = "lsw",
    version,
    about = "Linux Subsystem for Windows Development",
    long_about = "Build, run, and inspect Windows applications without leaving Linux."
)]
struct Cli {
    /// Verbose diagnostic logging.
    #[arg(long, global = true)]
    verbose: bool,
    /// Maximum-detail trace logging (implies --verbose).
    #[arg(long, global = true)]
    trace: bool,
    /// Output format for machine consumption where supported.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    format: Format,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Human,
    Json,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scaffold a new project (lsw.toml + CMake hello template).
    Init {
        /// Project name; omit to initialize the current directory.
        name: Option<String>,
    },
    /// Manage isolated Windows-target environments.
    #[command(subcommand)]
    Env(EnvCmd),
    /// Select the active environment for this project.
    Use { name: String },
    /// Build Windows artifacts using native Linux tools.
    Build {
        /// Force a build system (cmake, cargo, make, ninja, meson).
        #[arg(long)]
        system: Option<String>,
        /// Refresh lsw.lock instead of failing on drift.
        #[arg(long)]
        update_lock: bool,
    },
    /// Run an executable (PE via the Windows runtime, ELF natively).
    Run {
        program: PathBuf,
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
    /// Audit a PE's security hardening (ASLR, DEP, CFG, SafeSEH, signing).
    Audit { file: PathBuf },
    /// List the exported symbols of a PE (mirror of imports).
    Exports { file: PathBuf },
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
    #[command(subcommand)]
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
    },
    /// List the environment's Windows/runtime processes.
    Ps,
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
    /// Diagnose host, runtime, toolchain, and project health.
    Doctor,
    /// Generate shell completion scripts (bash, zsh, fish, powershell, elvish).
    Completions { shell: clap_complete::Shell },
    /// Explain an LSW#### error code.
    Explain { code: String },
}

#[derive(Subcommand)]
enum IdeCmd {
    /// Print the environment description IDE plugins consume (JSON).
    Env,
}

#[derive(Subcommand)]
enum PluginCmd {
    /// List discovered `lsw-provider-*` plugins and their handshake info.
    List,
}

#[derive(Subcommand)]
enum DaemonCmd {
    /// Show whether the daemon is running and its version.
    Status,
    /// Ask a running daemon to stop.
    Stop,
}

#[derive(Subcommand)]
enum RustCmd {
    /// Scaffold a Rust project wired for Windows targeting.
    Init { name: Option<String> },
    /// Report Rust->Windows toolchain readiness for the active environment.
    Doctor,
}

#[derive(Subcommand)]
enum ServiceCmd {
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
enum SdkCmd {
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
enum RegistryCmd {
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
enum EnvCmd {
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
enum SandboxArg {
    #[value(name = "strict")]
    Strict,
}

fn sandbox_from(a: Option<SandboxArg>) -> lsw_core::Sandbox {
    match a {
        Some(SandboxArg::Strict) => lsw_core::Sandbox::Strict,
        None => lsw_core::Sandbox::None,
    }
}

fn display_from(headless: bool) -> lsw_core::Display {
    if headless {
        lsw_core::Display::Headless
    } else {
        lsw_core::Display::Auto
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum PackageTargetArg {
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
enum ArchArg {
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

fn main() -> ExitCode {
    let cli = Cli::parse();

    let log_filter = if cli.trace {
        "trace"
    } else if cli.verbose {
        "debug"
    } else {
        "warn"
    };
    tracing_subscriber::fmt()
        .with_env_filter(log_filter)
        .with_writer(std::io::stderr)
        .init();

    match dispatch(&cli) {
        Ok(code) => code,
        Err(e) => {
            if cli.format == Format::Json {
                let payload = serde_json::json!({
                    "error": { "code": e.code(), "message": e.to_string() }
                });
                println!("{payload}");
            } else {
                eprintln!("error: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn cwd() -> PathBuf {
    std::env::current_dir().expect("current directory must exist")
}

fn project() -> lsw_core::Result<Project> {
    Project::discover(&cwd())
}

fn active_env(dirs: &Dirs) -> lsw_core::Result<(Project, lsw_core::Environment)> {
    let p = project()?;
    let env = lsw_core::resolve_active(dirs, &p)?;
    Ok((p, env))
}

fn dispatch(cli: &Cli) -> lsw_core::Result<ExitCode> {
    let dirs = Dirs::resolve()?;

    match &cli.command {
        Cmd::Init { name } => {
            let report = lsw_core::init(&cwd(), name.as_deref())?;
            println!("Initialized LSW project at {}", report.root.display());
            for f in &report.created {
                println!("  created {}", f.display());
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Env(EnvCmd::Create {
            name,
            arch,
            toolchain,
            sdk,
            force,
            expose_home,
        }) => {
            println!("Creating environment '{name}' (this initializes a Wine prefix)...");
            let report = lsw_core::env_create(
                &dirs,
                &EnvCreateOptions {
                    name: name.clone(),
                    arch: (*arch).into(),
                    toolchain: toolchain.clone(),
                    sdk: sdk.clone(),
                    force: *force,
                    expose_home: *expose_home,
                },
            )?;
            let m = &report.environment.manifest;
            println!("Environment '{name}' ready");
            println!("  arch      {}", m.target_arch);
            println!(
                "  toolchain {} {}",
                m.toolchain.provider, m.toolchain.version
            );
            println!("  runtime   {} {}", m.runtime.provider, m.runtime.version);
            println!("  probe     {}", report.probe.detail);
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Env(EnvCmd::List) => {
            let envs = lsw_core::env_list(&dirs)?;
            if envs.is_empty() {
                println!("No environments. Create one with: lsw env create <name>");
            }
            for e in envs {
                println!(
                    "{:<20} {:<8} {:<24} {:<16} {}",
                    e.name,
                    e.arch.to_string(),
                    e.toolchain,
                    e.runtime,
                    if e.healthy { "healthy" } else { "UNHEALTHY" }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Env(EnvCmd::Remove { name }) => {
            lsw_core::env_remove(&dirs, name)?;
            println!("Removed environment '{name}'");
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Env(EnvCmd::Clone { src, dst, force }) => {
            let env = lsw_core::clone_env(&dirs, src, dst, *force)?;
            println!("Cloned environment '{src}' to '{}'", env.name);
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Env(EnvCmd::Restore { name }) => {
            let p = project()?;
            println!("Restoring environment '{name}' from lsw.lock...");
            let report = lsw_core::env_restore(&dirs, &p, name)?;
            let m = &report.environment.manifest;
            println!("Environment '{name}' restored and verified against lsw.lock");
            println!("  arch      {}", m.target_arch);
            println!(
                "  toolchain {} {}",
                m.toolchain.provider, m.toolchain.version
            );
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Use { name } => {
            let mut p = project()?;
            lsw_core::use_environment(&dirs, &mut p, name)?;
            println!(
                "Project '{}' now uses environment '{name}'",
                p.manifest.project.name
            );
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Build {
            system,
            update_lock,
        } => {
            let (p, env) = active_env(&dirs)?;
            let report = lsw_core::build(
                &p,
                &env,
                &BuildOptions {
                    system: system.clone(),
                    update_lock: *update_lock,
                },
            )?;
            if cli.format == Format::Json {
                let payload = serde_json::json!({
                    "system": format!("{:?}", report.system),
                    "commands": report.commands,
                    "artifacts": report.artifacts,
                    "lock_written": report.lock_written,
                });
                println!("{payload}");
            } else {
                println!("Build OK ({:?})", report.system);
                for a in &report.artifacts {
                    println!("  {}", a.display());
                }
                if report.lock_written {
                    println!("  wrote lsw.lock");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Run {
            program,
            args,
            host,
            windows,
            sandbox,
            headless,
        } => {
            let (p, env) = active_env(&dirs)?;
            let domain = domain_from_flags(*host, *windows);
            let report = lsw_core::run(
                &env,
                Some(&p),
                program,
                args,
                domain,
                sandbox_from(*sandbox),
                display_from(*headless),
            )?;
            note_runtime_domain(&report);
            Ok(exit_from_status(report.status))
        }

        Cmd::Exec {
            host,
            windows,
            sandbox,
            headless,
            command,
        } => {
            let (p, env) = active_env(&dirs)?;
            let domain = domain_from_flags(*host, *windows);
            let (program, args) = command.split_first().expect("clap enforces non-empty");
            let report = lsw_core::run(
                &env,
                Some(&p),
                &PathBuf::from(program),
                args,
                domain,
                sandbox_from(*sandbox),
                display_from(*headless),
            )?;
            note_runtime_domain(&report);
            Ok(exit_from_status(report.status))
        }

        Cmd::Test { headless } => {
            let (p, env) = active_env(&dirs)?;
            let report = lsw_core::test(
                &p,
                &env,
                &lsw_core::TestOptions {
                    headless: *headless,
                },
            )?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("report serializes")
                );
            } else {
                let outcome = |o: lsw_core::Outcome| match o {
                    lsw_core::Outcome::Pass => "PASS",
                    lsw_core::Outcome::Fail => "FAIL",
                    lsw_core::Outcome::NotRun => "NOT RUN",
                };
                println!("\nLSW Test Report\n");
                println!("Build:");
                println!(
                    "  {:<24} {}",
                    report.build.label,
                    outcome(report.build.outcome)
                );
                println!("Runtime:");
                println!(
                    "  {:<24} {}",
                    report.runtime.label,
                    outcome(report.runtime.outcome)
                );
                println!("Native:");
                println!(
                    "  {:<24} {}",
                    report.native.label,
                    outcome(report.native.outcome)
                );
                if let (Some(p), Some(f)) = (report.tests_passed, report.tests_failed) {
                    println!("\nTests:\n  {p} passed, {f} failed");
                }
                let compat = match report.compatibility {
                    lsw_core::CompatStatus::LocalCompatibilityVerified => {
                        "LOCAL_COMPATIBILITY_VERIFIED"
                    }
                    lsw_core::CompatStatus::LocalCompatibilityFailed => {
                        "LOCAL_COMPATIBILITY_FAILED"
                    }
                    lsw_core::CompatStatus::NotRun => "NOT_RUN",
                };
                println!("\nCompatibility status:\n  {compat}");
            }
            Ok(
                if report.compatibility == lsw_core::CompatStatus::LocalCompatibilityVerified {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                },
            )
        }

        Cmd::Verify { native_windows } => {
            let (p, env) = active_env(&dirs)?;
            if !*native_windows {
                eprintln!("note: only --native-windows verification is supported");
            }
            let report = lsw_core::verifyops::verify(&p, &env)?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                let status = match report.status {
                    lsw_core::verifyops::VerifyStatus::WindowsVerified => "WINDOWS_VERIFIED",
                    lsw_core::verifyops::VerifyStatus::WindowsFailed => "WINDOWS_FAILED",
                    lsw_core::verifyops::VerifyStatus::WindowsUnavailable => "WINDOWS_UNAVAILABLE",
                };
                println!(
                    "Native verification host: {}",
                    report.host.as_deref().unwrap_or("none")
                );
                for r in &report.results {
                    println!("  {:<24} exit {:?}", r.artifact, r.exit_code);
                }
                println!("Status: {status}");
                println!("{}", report.detail);
            }
            Ok(match report.status {
                lsw_core::verifyops::VerifyStatus::WindowsVerified => ExitCode::SUCCESS,
                lsw_core::verifyops::VerifyStatus::WindowsUnavailable => ExitCode::SUCCESS,
                lsw_core::verifyops::VerifyStatus::WindowsFailed => ExitCode::FAILURE,
            })
        }

        Cmd::Shell { windows } => {
            let (p, env) = active_env(&dirs)?;
            if !*windows {
                println!("Entering LSW shell (env: {}); 'exit' to leave.", env.name);
            }
            let status = lsw_core::shell(&env, Some(&p), *windows)?;
            Ok(exit_from_status(status))
        }

        Cmd::Audit { file } => {
            let report = lsw_core::auditops::audit(file)?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                println!("\nLSW AUDIT  {}\n", file.display());
                for c in &report.checks {
                    let mark = if c.enabled { "+" } else { "X" };
                    println!("  {mark} {:<22} {}", c.name, c.detail);
                }
                println!(
                    "\n{}",
                    if report.hardened {
                        "baseline hardening present (ASLR + DEP)"
                    } else {
                        "WEAK: missing ASLR or DEP"
                    }
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Exports { file } => {
            let names = lsw_core::auditops::exports(file)?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&names).expect("serializes")
                );
            } else if names.is_empty() {
                println!("no exports (not a DLL, or no export table)");
            } else {
                for n in &names {
                    println!("{n}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Inspect { file } => {
            let env = active_env(&dirs).ok().map(|(_, e)| e);
            let report = lsw_core::inspect(file, env.as_ref())?;
            if cli.format == Format::Json {
                let imports: Vec<_> = report
                    .imports
                    .iter()
                    .map(|i| serde_json::json!({ "dll": i.dll, "available": i.available }))
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({
                        "format": format!("{:?}", report.info.format),
                        "machine": format!("{:?}", report.info.machine),
                        "subsystem": format!("{:?}", report.info.subsystem),
                        "imports": imports,
                    })
                );
            } else {
                println!("Format:    {:?}", report.info.format);
                println!("Machine:   {:?}", report.info.machine);
                println!("Subsystem: {:?}", report.info.subsystem);
                println!("Imports:");
                for i in &report.imports {
                    let availability = match i.available {
                        Some(true) => "available",
                        Some(false) => "MISSING in runtime",
                        None => "unknown (no environment)",
                    };
                    println!("  {:<24} {}", i.dll, availability);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Path { windows, linux } => {
            let (p, env) = active_env(&dirs)?;
            let mapper = lsw_core::mapper(&env, &p);
            match (windows, linux) {
                (Some(path), None) => {
                    let absolute = if path.is_absolute() {
                        path.clone()
                    } else {
                        cwd().join(path)
                    };
                    println!("{}", mapper.to_windows(&absolute)?);
                }
                (None, Some(text)) => {
                    println!("{}", mapper.to_linux(text)?.display());
                }
                _ => {
                    eprintln!("usage: lsw path --windows <linux-path> | --linux <windows-path>");
                    return Ok(ExitCode::FAILURE);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Registry(op) => {
            let (p, env) = active_env(&dirs)?;
            match op {
                RegistryCmd::Get { key, value } => {
                    lsw_core::registryops::get(&env, key, value.as_deref())?;
                }
                RegistryCmd::Set {
                    key,
                    value,
                    data,
                    kind,
                } => {
                    lsw_core::registryops::set(&env, key, value, data, kind)?;
                    println!("set {key}\\{value}");
                }
                RegistryCmd::Export { key, file } => {
                    lsw_core::registryops::export(&env, key, file)?;
                    println!("exported {key} to {}", file.display());
                }
                RegistryCmd::Import { file } => {
                    lsw_core::registryops::import(&env, file)?;
                    println!("imported {}", file.display());
                }
                RegistryCmd::Seed => {
                    let n = lsw_core::registryops::seed(&env, &p)?;
                    println!("applied {n} registry seed(s) to '{}'", env.name);
                }
                RegistryCmd::Reset => {
                    lsw_core::registryops::reset(&env)?;
                    println!("registry reset to prefix defaults for '{}'", env.name);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Debug {
            program,
            args,
            gdb,
            no_start,
        } => {
            let (p, env) = active_env(&dirs)?;
            let status = lsw_core::debugops::debug(
                &env,
                Some(&p),
                program,
                args,
                &lsw_core::debugops::DebugOptions {
                    gdb: *gdb,
                    no_start: *no_start,
                },
            )?;
            Ok(exit_from_status(status))
        }

        Cmd::Dap => {
            let (_p, env) = active_env(&dirs)?;
            let stdin = std::io::stdin();
            let mut reader = stdin.lock();
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            lsw_core::dapops::serve(&env, &mut reader, &mut writer)?;
            Ok(ExitCode::SUCCESS)
        }

        Cmd::CompatQuery { key } => {
            let db = lsw_core::compatdb::CompatDb::load(&dirs)?;
            match db.query(key) {
                Some(entry) => {
                    let verdict = match entry.verdict() {
                        lsw_core::compatdb::Verdict::Supported => "SUPPORTED",
                        lsw_core::compatdb::Verdict::Unsupported => "UNSUPPORTED",
                    };
                    if cli.format == Format::Json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(entry).expect("serializes")
                        );
                    } else {
                        println!("{key}: {verdict}");
                        println!(
                            "  observed supported {} / unsupported {} (last: {})",
                            entry.supported_count, entry.unsupported_count, entry.last_runtime
                        );
                    }
                }
                None => println!(
                    "{key}: not in the compatibility database yet (run `lsw compat --db <app.exe>`)"
                ),
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Compat { program, args, db } => {
            let (_p, env) = active_env(&dirs)?;
            let report = if *db {
                lsw_core::compatops::compat_recording(&env, program, args, &dirs)?
            } else {
                lsw_core::compatops::compat(&env, program, args)?
            };
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                println!("\nLSW Compatibility Report  {}\n", program.display());
                println!("Required imported DLLs:  {}", report.imported_dlls);
                println!("Imported API functions:  {}", report.imported_functions);
                println!("Loaded at runtime:       {}", report.loaded_dlls);
                println!("Supported locally:       {}", report.supported_locally);
                println!(
                    "Potentially unsupported: {}",
                    report.potentially_unsupported.len()
                );
                for d in &report.potentially_unsupported {
                    println!("  ? {d}");
                }
                if !report.unsupported_apis.is_empty() {
                    println!("Unimplemented APIs:");
                    for a in &report.unsupported_apis {
                        println!("  X {a}");
                    }
                }
                println!("\nFeature                Local");
                println!("---------------------------------");
                for c in &report.capabilities {
                    let s = match c.local {
                        lsw_core::compatops::Support::Yes => "yes",
                        lsw_core::compatops::Support::Partial => "partial",
                        lsw_core::compatops::Support::No => "no",
                        lsw_core::compatops::Support::Unused => "-",
                    };
                    println!("{:<22} {}", c.feature, s);
                }
                println!("\n{}", report.note);
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Trace {
            program,
            args,
            relay,
        } => {
            let (_p, env) = active_env(&dirs)?;
            let report = lsw_core::traceops::trace(
                &env,
                program,
                args,
                &lsw_core::traceops::TraceOptions { relay: *relay },
            )?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                println!("\nLSW TRACE  {}\n", program.display());
                println!("Imported DLLs: {}", report.imported_dlls.len());
                println!("Loaded DLLs:   {}", report.loaded_dlls.len());
                for d in &report.loaded_dlls {
                    println!("  + {d}");
                }
                if !report.observed_calls.is_empty() {
                    println!("Observed API calls: {}", report.observed_calls.len());
                    for c in &report.observed_calls {
                        println!("  + {c}");
                    }
                }
                if !report.registry_access.is_empty() {
                    println!("Registry operations: {}", report.registry_access.len());
                    for r in &report.registry_access {
                        println!("  R {r}");
                    }
                }
                if !report.filesystem_access.is_empty() {
                    println!("Filesystem operations: {}", report.filesystem_access.len());
                    for f in &report.filesystem_access {
                        println!("  F {f}");
                    }
                }
                if report.unsupported.is_empty() {
                    println!("Unsupported APIs: none observed");
                } else {
                    println!("Unsupported APIs:");
                    for u in &report.unsupported {
                        println!("  X {u}");
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Package { target } => {
            let (p, env) = active_env(&dirs)?;
            let target = match target {
                PackageTargetArg::PortableDirectory => {
                    lsw_core::packageops::PackageTarget::PortableDirectory
                }
                PackageTargetArg::Zip => lsw_core::packageops::PackageTarget::Zip,
                PackageTargetArg::Msi => lsw_core::packageops::PackageTarget::Msi,
                PackageTargetArg::Msix => lsw_core::packageops::PackageTarget::Msix,
            };
            let report = lsw_core::packageops::package(&p, &env, target)?;
            println!("Packaged: {}", report.directory.display());
            for f in &report.files {
                println!("  {f}");
            }
            if let Some(zip) = &report.zip {
                println!("Archive:  {}", zip.display());
            }
            if let Some(msi) = &report.msi {
                println!("Installer: {}", msi.display());
            }
            if let Some(msix) = &report.msix {
                println!("MSIX:      {} (self-signed)", msix.display());
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Ps => {
            let (_p, env) = active_env(&dirs)?;
            let processes = lsw_core::psops::ps(&env)?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&processes).expect("serializes")
                );
            } else if processes.is_empty() {
                println!("No processes running in environment '{}'", env.name);
            } else {
                println!("{:<8} COMMAND", "PID");
                for p in processes {
                    println!("{:<8} {}", p.pid, p.command);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Kill { pid, all } => {
            let (_p, env) = active_env(&dirs)?;
            if *all {
                lsw_core::psops::kill_all(&env)?;
                println!("environment '{}' shut down", env.name);
            } else if let Some(pid) = pid {
                lsw_core::psops::kill(&env, *pid)?;
                println!("sent SIGTERM to {pid}");
            } else {
                eprintln!("usage: lsw kill <pid> | lsw kill --all");
                return Ok(ExitCode::FAILURE);
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Service(op) => {
            let (_p, env) = active_env(&dirs)?;
            match op {
                ServiceCmd::Create { name, bin } => {
                    lsw_core::serviceops::create(&env, name, bin)?;
                    println!("created service '{name}'");
                }
                ServiceCmd::Start { name } => {
                    lsw_core::serviceops::start(&env, name)?;
                    println!("started service '{name}'");
                }
                ServiceCmd::Stop { name } => {
                    lsw_core::serviceops::stop(&env, name)?;
                    println!("stopped service '{name}'");
                }
                ServiceCmd::Query { name } => {
                    let status = lsw_core::serviceops::query(&env, name)?;
                    if cli.format == Format::Json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&status).expect("serializes")
                        );
                    } else {
                        println!("{:<24} {}", status.name, status.state);
                    }
                }
                ServiceCmd::Delete { name } => {
                    lsw_core::serviceops::delete(&env, name)?;
                    println!("deleted service '{name}'");
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Rust(RustCmd::Init { name }) => {
            let report = lsw_core::rustops::init(&cwd(), name.as_deref())?;
            println!("Initialized LSW Rust project at {}", report.root.display());
            for f in &report.created {
                println!("  created {}", f.display());
            }
            println!("Next: lsw env create <name> && lsw build");
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Rust(RustCmd::Doctor) => {
            let (_p, env) = active_env(&dirs)?;
            let report = lsw_core::rustops::doctor(&env)?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializes")
                );
            } else {
                let mark = |c: lsw_core::rustops::Check| match c {
                    lsw_core::rustops::Check::Ok => "OK",
                    lsw_core::rustops::Check::NotConfigured => "NOT CONFIGURED",
                    lsw_core::rustops::Check::Missing => "MISSING",
                };
                println!("LSW Rust Doctor  (target {})\n", report.target);
                println!("  Compiler target   {}", mark(report.compiler_target));
                println!("  Linker            {}", mark(report.linker));
                println!("  CRT               {}", mark(report.crt));
                println!("  Windows imports   {}", mark(report.windows_imports));
                println!("  Runtime execution {}", mark(report.runtime_execution));
                println!("  Native validation {}", mark(report.native_validation));
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Daemon(DaemonCmd::Status) => {
            let probe = lsw_core::daemonops::DaemonClient::connect(&dirs)
                .and_then(|mut c| c.call("version"));
            match probe {
                Ok(v) => println!(
                    "lswd running (protocol v{}, version {})",
                    v["protocol"], v["version"]
                ),
                Err(_) => println!("lswd not running (start it with: lswd)"),
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Daemon(DaemonCmd::Stop) => {
            let mut client = lsw_core::daemonops::DaemonClient::connect(&dirs)?;
            client.call("shutdown")?;
            println!("lswd stopping");
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Plugin(PluginCmd::List) => {
            let discovered = lsw_core::pluginops::discover();
            if discovered.is_empty() {
                println!(
                    "No provider plugins found (looked for lsw-provider-* on PATH, protocol v{})",
                    lsw_core::pluginops::PROTOCOL_VERSION
                );
            }
            for d in discovered {
                match lsw_core::pluginops::Plugin::connect(&d.name, &d.path) {
                    Ok(plugin) => {
                        let h = &plugin.handshake;
                        println!(
                            "{:<16} {:<10} {:<8} proto v{}  {}",
                            d.name,
                            h.provider_version,
                            h.kind,
                            h.protocol,
                            d.path.display()
                        );
                        plugin.shutdown();
                    }
                    Err(e) => println!("{:<16} ERROR: {e}", d.name),
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Sdk(SdkCmd::Import { name, from, force }) => {
            println!("Importing SDK '{name}' from {}...", from.display());
            println!(
                "Note: you are responsible for the license terms of any Microsoft SDK content you import."
            );
            let report = lsw_core::sdkops::import(&dirs, name, from, *force)?;
            println!(
                "Imported '{}' ({} files) to {}",
                report.name,
                report.files_copied,
                report.root.display()
            );
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Sdk(SdkCmd::List) => {
            let sdks = lsw_core::sdkops::list(&dirs)?;
            if sdks.is_empty() {
                println!("No SDKs imported. Import one with: lsw sdk import <name> --from <path>");
            }
            for s in sdks {
                println!(
                    "{:<20} {:<10} {}",
                    s.name,
                    if s.usable { "usable" } else { "incomplete" },
                    s.source.display()
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Sdk(SdkCmd::Remove { name }) => {
            lsw_core::sdkops::remove(&dirs, name)?;
            println!("Removed SDK '{name}'");
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Ide(IdeCmd::Env) => {
            let (p, env) = active_env(&dirs)?;
            let description = lsw_core::ideops::ide_env(&env, Some(&p))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&description).expect("serializes")
            );
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Doctor => {
            let p = project().ok();
            let report = lsw_core::doctor(&dirs, p.as_ref())?;
            if cli.format == Format::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("report serializes")
                );
            } else {
                println!("LSW Environment Doctor\n");
                for section in &report.sections {
                    println!("{}", section.name);
                    for r in &section.rows {
                        let status = match r.status {
                            Status::Ok => "OK",
                            Status::Warn => "WARNING",
                            Status::Fail => "FAIL",
                        };
                        println!("  {:<18} {:<52} {}", r.label, r.value, status);
                    }
                    println!();
                }
                println!(
                    "Overall: {}",
                    if report.healthy {
                        "healthy"
                    } else {
                        "PROBLEMS FOUND"
                    }
                );
            }
            Ok(if report.healthy {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }

        Cmd::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(*shell, &mut cmd, "lsw", &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }

        Cmd::Explain { code } => match lsw_core::explainops::explain(code) {
            Some(e) => {
                println!("{}  {}", e.code, e.summary);
                println!("  fix: {}", e.hint);
                Ok(ExitCode::SUCCESS)
            }
            None => {
                eprintln!(
                    "no explanation for '{code}' (try an LSW#### code from an error message)"
                );
                Ok(ExitCode::FAILURE)
            }
        },
    }
}

/// Honesty marker: local runtime success must never read as
/// native Windows success.
fn note_runtime_domain(report: &lsw_core::RunReport) {
    if report.domain == Domain::Windows {
        eprintln!(
            "[lsw] executed via local compatibility runtime (wine) - not verified on native Windows"
        );
    }
}

fn domain_from_flags(host: bool, windows: bool) -> Domain {
    if host {
        Domain::Host
    } else if windows {
        Domain::Windows
    } else {
        Domain::Auto
    }
}

fn exit_from_status(status: std::process::ExitStatus) -> ExitCode {
    match status.code() {
        Some(code) => ExitCode::from(code.clamp(0, 255) as u8),
        None => ExitCode::FAILURE,
    }
}
