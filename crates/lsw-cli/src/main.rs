use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use lsw_core::{Dirs, Domain, Project};

use crate::cli::{Cli, Cmd, Format};

mod cli;
mod cmd;
mod color;
mod install;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(name) = &cli.env {
        let _ = ENV_OVERRIDE.set(name.clone());
    }
    color::set_mode(match cli.color {
        cli::ColorMode::Auto => None,
        cli::ColorMode::Always => Some(true),
        cli::ColorMode::Never => Some(false),
    });

    let log_filter = if cli.trace {
        "trace"
    } else if cli.verbose {
        "debug,minidump::context=error"
    } else {
        "warn,minidump::context=error"
    };
    tracing_subscriber::fmt()
        .with_env_filter(log_filter)
        .with_writer(std::io::stderr)
        .init();

    note_format_gaps(&cli);

    match dispatch(&cli) {
        Ok(code) => code,
        Err(e) => {
            if cli.format == Format::Json {
                cmd::emit_json(&serde_json::json!({
                    "error": { "code": e.code(), "message": e.to_string() }
                }));
            } else {
                eprintln!("error: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

fn note_format_gaps(cli: &Cli) {
    use cli::Cmd;
    if cli.format == Format::Human {
        return;
    }
    let json_unsupported = matches!(
        &cli.command,
        Cmd::Init { .. }
            | Cmd::Use { .. }
            | Cmd::Run { .. }
            | Cmd::Exec { .. }
            | Cmd::Shell { .. }
            | Cmd::Sign { .. }
            | Cmd::Kill { .. }
            | Cmd::Dap
            | Cmd::Ide(_)
            | Cmd::Watch { .. }
            | Cmd::Clean { .. }
            | Cmd::Completions { .. }
            | Cmd::Man { .. }
            | Cmd::Install { .. }
    );
    if json_unsupported {
        eprintln!("note: --format has no effect on this command; output stays human-readable");
    } else if cli.format == Format::Csv && !matches!(&cli.command, Cmd::Trace { .. }) {
        eprintln!("note: --format csv applies to lsw trace only; falling back to human output");
    }
}

pub(crate) fn cwd() -> lsw_core::Result<PathBuf> {
    std::env::current_dir().map_err(|e| lsw_core::Error::io(PathBuf::from("."), e))
}

pub(crate) fn usage_failure(format: Format, message: &str) -> ExitCode {
    if format == Format::Json {
        cmd::emit_json(&serde_json::json!({ "error": { "code": "LSW0000", "message": message } }));
    } else {
        eprintln!("error: {message}");
    }
    ExitCode::FAILURE
}

pub(crate) fn print_dep_tree(node: &lsw_core::depsops::DepNode, depth: usize) {
    use lsw_core::depsops::DepKind;
    let tag = match node.kind {
        DepKind::Root | DepKind::Resolved => String::new(),
        DepKind::System => color::dim("  [system]"),
        DepKind::Missing => color::red("  [MISSING]"),
        DepKind::Seen => color::dim("  [seen]"),
    };
    println!("{}{}{}", "  ".repeat(depth), node.name, tag);
    for child in &node.children {
        print_dep_tree(child, depth + 1);
    }
}

pub(crate) fn project() -> lsw_core::Result<Project> {
    Project::discover(&cwd()?)
}

static ENV_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub(crate) fn env_override() -> Option<&'static str> {
    ENV_OVERRIDE.get().map(String::as_str)
}

pub(crate) fn active_env(dirs: &Dirs) -> lsw_core::Result<(Project, lsw_core::Environment)> {
    let p = project()?;
    let env = match env_override() {
        Some(name) => lsw_core::Environment::open(dirs, name)?,
        None => lsw_core::resolve_active(dirs, &p)?,
    };
    Ok((p, env))
}

pub(crate) fn admin_env(dirs: &Dirs) -> lsw_core::Result<lsw_core::Environment> {
    match env_override() {
        Some(name) => lsw_core::Environment::open(dirs, name),
        None => active_env(dirs).map(|(_, env)| env),
    }
}

fn with_pe<F>(
    file: &Option<PathBuf>,
    dirs: &Dirs,
    format: Format,
    f: F,
) -> lsw_core::Result<ExitCode>
where
    F: FnOnce(&std::path::Path) -> lsw_core::Result<ExitCode>,
{
    match cmd::resolve_pe(file, dirs)? {
        Some(p) => f(&p),
        None => {
            if format == Format::Json {
                cmd::emit_json(&serde_json::json!({
                    "error": { "code": "LSW0000", "message": "no single artifact to analyze; pass a file explicitly" }
                }));
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

fn dispatch(cli: &Cli) -> lsw_core::Result<ExitCode> {
    let dirs = Dirs::resolve()?;

    match &cli.command {
        Cmd::Setup => cmd::project::setup(&dirs, cli.format),
        Cmd::Init { name, template } => cmd::project::init(name, template),
        Cmd::Env(op) => cmd::project::env(op, &dirs, cli.format),
        Cmd::Use { name } => cmd::project::use_env(name, &dirs),
        Cmd::Build {
            system,
            update_lock,
            reproducible,
            aot,
        } => cmd::build::build(system, update_lock, reproducible, aot, &dirs, cli.format),
        Cmd::Run {
            program,
            args,
            domain,
            dump_on_crash,
        } => cmd::build::run(program, args, domain, dump_on_crash, &dirs),
        Cmd::Exec { domain, command } => cmd::build::exec(domain, command, &dirs),
        Cmd::Test {
            headless,
            junit,
            coverage,
        } => cmd::build::test(headless, junit, *coverage, &dirs, cli.format),
        Cmd::Check { headless } => cmd::tooling::check(*headless, &dirs, cli.format),
        Cmd::Verify {
            native,
            reproducible,
            artifact,
        } => cmd::verify::verify(native, reproducible, artifact, &dirs, cli.format),
        Cmd::Shell { windows } => cmd::build::shell(windows, &dirs),
        Cmd::Inspect { file } => with_pe(file, &dirs, cli.format, |f| {
            cmd::inspect::inspect(f, &dirs, cli.format)
        }),
        Cmd::Crash { file, force } => cmd::inspect::crash(file, *force, &dirs, cli.format),
        Cmd::Audit { file } => with_pe(file, &dirs, cli.format, |f| {
            cmd::inspect::audit(f, cli.format)
        }),
        Cmd::Exports { file } => with_pe(file, &dirs, cli.format, |f| {
            cmd::inspect::exports(f, cli.format)
        }),
        Cmd::Sbom { file } => with_pe(file, &dirs, cli.format, cmd::inspect::sbom),
        Cmd::Diff { a, b } => cmd::inspect::diff(a, b, cli.format),
        Cmd::Size {
            file,
            baseline,
            max_growth,
        } => with_pe(file, &dirs, cli.format, |f| {
            cmd::inspect::size(f, baseline, max_growth, cli.format)
        }),
        Cmd::Strings { file, min } => with_pe(file, &dirs, cli.format, |f| {
            cmd::inspect::strings(f, min, cli.format)
        }),
        Cmd::Deps(op) => cmd::inspect::deps(op, &dirs, cli.format),
        Cmd::Ci(op) => cmd::config::ci(op, cli.format),
        Cmd::Config(op) => cmd::config::config(op, cli.format),
        Cmd::Sign {
            file,
            publisher,
            pfx,
            pfx_pass_env,
            timestamp_url,
            verify,
        } => cmd::package::sign(file, publisher, pfx, pfx_pass_env, timestamp_url, *verify),
        Cmd::Path {
            to_windows,
            to_linux,
        } => cmd::package::path(to_windows, to_linux, &dirs, cli.format),
        Cmd::Registry(op) => cmd::state::registry(op, &dirs, cli.format),
        Cmd::Debug {
            program,
            args,
            attach,
            gdb,
            no_start,
            native,
            analyze,
            interactive,
        } => cmd::debug::debug(
            program,
            args,
            &cmd::debug::DebugFlags {
                attach: *attach,
                gdb: *gdb,
                no_start: *no_start,
                native: *native,
                analyze: *analyze,
                interactive: *interactive,
            },
            &dirs,
            cli.format,
        ),
        Cmd::Dap => cmd::debug::dap(&dirs),
        Cmd::Compat {
            program,
            args,
            db,
            native,
        } => with_pe(program, &dirs, cli.format, |f| {
            cmd::verify::compat(f, args, db, native, &dirs, cli.format)
        }),
        Cmd::CompatQuery { key } => cmd::verify::compat_query(key, &dirs, cli.format),
        Cmd::Trace {
            program,
            args,
            relay,
            filter,
        } => with_pe(program, &dirs, cli.format, |f| {
            cmd::verify::trace(f, args, relay, filter, &dirs, cli.format)
        }),
        Cmd::Package {
            target,
            verify,
            bundle_deps,
        } => cmd::package::package(target, *verify, *bundle_deps, &dirs, cli.format),
        Cmd::Ps { all } => cmd::state::ps(*all, &dirs, cli.format),
        Cmd::Kill { pid, all } => cmd::state::kill(pid, all, &dirs),
        Cmd::Service(op) => cmd::state::service(op, &dirs, cli.format),
        Cmd::Rust(op) => cmd::lang::rust(op, &dirs, cli.format),
        Cmd::Dotnet(op) => cmd::lang::dotnet(op, &dirs, cli.format),
        Cmd::Daemon(op) => cmd::integration::daemon(op, &dirs, cli.format),
        Cmd::Plugin(op) => cmd::integration::plugin(op, cli.format),
        Cmd::Sdk(op) => cmd::lang::sdk(op, &dirs, cli.format),
        Cmd::Ide(op) => cmd::integration::ide(op, &dirs),
        Cmd::Toolchain(op) => cmd::tooling::toolchain(op, &dirs, cli.format),
        Cmd::Watch { run, test } => cmd::tooling::watch(*run, *test, &dirs),
        Cmd::Clean { deps } => cmd::tooling::clean(*deps),
        Cmd::Doctor => cmd::tooling::doctor(&dirs, cli.format),
        Cmd::Completions { shell } => cmd::tooling::completions(shell),
        Cmd::Man { dir } => cmd::tooling::man(dir),
        Cmd::Install { prefix } => cmd::tooling::install(prefix),
        Cmd::Explain { code } => cmd::tooling::explain(code, cli.format),
    }
}

/// Honesty marker: local runtime success must never read as
/// native Windows success.
pub(crate) fn note_crash(status: &std::process::ExitStatus) {
    if let Some(code) = status.code()
        && let Some(reason) = lsw_core::verifyops::crash_reason(code)
    {
        eprintln!("[lsw] program crashed: {reason} (exit code {code:#x})");
    }
}

pub(crate) fn note_runtime_domain(report: &lsw_core::RunReport) {
    if report.domain == Domain::Windows {
        eprintln!(
            "[lsw] executed via local compatibility runtime (wine) - not verified on native Windows"
        );
    }
}

pub(crate) fn exit_from_status(status: std::process::ExitStatus) -> ExitCode {
    match status.code() {
        Some(code) => ExitCode::from(code.clamp(0, 255) as u8),
        None => ExitCode::FAILURE,
    }
}
