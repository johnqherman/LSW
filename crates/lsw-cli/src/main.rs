use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
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
        /// Force a build system ("cmake").
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
    },
    /// Run a command in an explicit execution domain.
    Exec {
        /// Host (Linux) domain.
        #[arg(long, conflicts_with = "windows")]
        host: bool,
        /// Windows domain.
        #[arg(long)]
        windows: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Interactive shell: Linux with Windows-target env, or cmd.exe.
    Shell {
        /// Launch an actual Windows shell (cmd.exe) in the environment.
        #[arg(long)]
        windows: bool,
    },
    /// Inspect a PE binary: format, architecture, subsystem, imports.
    Inspect { file: PathBuf },
    /// Translate paths between Linux and Windows views.
    Path {
        /// Print the Windows form of a Linux path.
        #[arg(long, conflicts_with = "linux")]
        windows: Option<PathBuf>,
        /// Print the Linux form of a Windows path.
        #[arg(long)]
        linux: Option<String>,
    },
    /// Diagnose host, runtime, toolchain, and project health.
    Doctor,
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
        /// Recreate if it already exists.
        #[arg(long)]
        force: bool,
    },
    /// List environments.
    List,
    /// Delete an environment and its Wine prefix.
    Remove { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum ArchArg {
    #[value(name = "x86_64")]
    X86_64,
    #[value(name = "x86")]
    X86,
    #[value(name = "aarch64")]
    Aarch64,
}

impl From<ArchArg> for TargetArch {
    fn from(a: ArchArg) -> Self {
        match a {
            ArchArg::X86_64 => TargetArch::X86_64,
            ArchArg::X86 => TargetArch::X86,
            ArchArg::Aarch64 => TargetArch::Aarch64,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(if cli.verbose { "debug" } else { "warn" })
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
            force,
        }) => {
            println!("Creating environment '{name}' (this initializes a Wine prefix)...");
            let report = lsw_core::env_create(
                &dirs,
                &EnvCreateOptions {
                    name: name.clone(),
                    arch: (*arch).into(),
                    toolchain: toolchain.clone(),
                    force: *force,
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
        } => {
            let (p, env) = active_env(&dirs)?;
            let domain = domain_from_flags(*host, *windows);
            let report = lsw_core::run(&env, Some(&p), program, args, domain)?;
            note_runtime_domain(&report);
            Ok(exit_from_status(report.status))
        }

        Cmd::Exec {
            host,
            windows,
            command,
        } => {
            let (p, env) = active_env(&dirs)?;
            let domain = domain_from_flags(*host, *windows);
            let (program, args) = command.split_first().expect("clap enforces non-empty");
            let report = lsw_core::run(&env, Some(&p), &PathBuf::from(program), args, domain)?;
            note_runtime_domain(&report);
            Ok(exit_from_status(report.status))
        }

        Cmd::Shell { windows } => {
            let (p, env) = active_env(&dirs)?;
            if !*windows {
                println!("Entering LSW shell (env: {}); 'exit' to leave.", env.name);
            }
            let status = lsw_core::shell(&env, Some(&p), *windows)?;
            Ok(exit_from_status(status))
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
