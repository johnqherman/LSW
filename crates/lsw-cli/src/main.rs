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

pub(crate) fn cwd() -> PathBuf {
    std::env::current_dir().expect("current directory must exist")
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
    Project::discover(&cwd())
}

pub(crate) fn active_env(dirs: &Dirs) -> lsw_core::Result<(Project, lsw_core::Environment)> {
    let p = project()?;
    let env = lsw_core::resolve_active(dirs, &p)?;
    Ok((p, env))
}

fn dispatch(cli: &Cli) -> lsw_core::Result<ExitCode> {
    let dirs = Dirs::resolve()?;

    match &cli.command {
        Cmd::Init { name, template } => cmd::project::init(name, template),
        Cmd::Env(op) => cmd::project::env(op, &dirs),
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
            host,
            windows,
            sandbox,
            headless,
        } => cmd::build::run(program, args, host, windows, sandbox, headless, &dirs),
        Cmd::Exec {
            host,
            windows,
            sandbox,
            headless,
            command,
        } => cmd::build::exec(host, windows, sandbox, headless, command, &dirs),
        Cmd::Test { headless } => cmd::build::test(headless, &dirs, cli.format),
        Cmd::Verify { native_windows } => cmd::verify::verify(native_windows, &dirs, cli.format),
        Cmd::Shell { windows } => cmd::build::shell(windows, &dirs),
        Cmd::Inspect { file } => cmd::inspect::inspect(file, &dirs, cli.format),
        Cmd::Crash { file } => cmd::inspect::crash(file, cli.format),
        Cmd::Audit { file } => cmd::inspect::audit(file, cli.format),
        Cmd::Exports { file } => cmd::inspect::exports(file, cli.format),
        Cmd::Sbom { file } => cmd::inspect::sbom(file),
        Cmd::Diff { a, b } => cmd::inspect::diff(a, b, cli.format),
        Cmd::Strings { file, min } => cmd::inspect::strings(file, min),
        Cmd::Deps(op) => cmd::inspect::deps(op, &dirs, cli.format),
        Cmd::Ci(op) => cmd::config::ci(op),
        Cmd::Config(op) => cmd::config::config(op, cli.format),
        Cmd::Sign { file, publisher } => cmd::package::sign(file, publisher),
        Cmd::Path { windows, linux } => cmd::package::path(windows, linux, &dirs),
        Cmd::Registry(op) => cmd::state::registry(op, &dirs),
        Cmd::Debug {
            program,
            args,
            gdb,
            no_start,
            native,
        } => cmd::debug::debug(program, args, gdb, no_start, native, &dirs, cli.format),
        Cmd::Dap => cmd::debug::dap(&dirs),
        Cmd::Compat {
            program,
            args,
            db,
            native,
        } => cmd::verify::compat(program, args, db, native, &dirs, cli.format),
        Cmd::CompatQuery { key } => cmd::verify::compat_query(key, &dirs, cli.format),
        Cmd::Trace {
            program,
            args,
            relay,
        } => cmd::verify::trace(program, args, relay, &dirs, cli.format),
        Cmd::Package { target, verify } => cmd::package::package(target, *verify, &dirs),
        Cmd::Ps => cmd::state::ps(&dirs, cli.format),
        Cmd::Kill { pid, all } => cmd::state::kill(pid, all, &dirs),
        Cmd::Service(op) => cmd::state::service(op, &dirs, cli.format),
        Cmd::Rust(op) => cmd::lang::rust(op, &dirs, cli.format),
        Cmd::Dotnet(op) => cmd::lang::dotnet(op, &dirs, cli.format),
        Cmd::Daemon(op) => cmd::integration::daemon(op, &dirs),
        Cmd::Plugin(op) => cmd::integration::plugin(op),
        Cmd::Sdk(op) => cmd::lang::sdk(op, &dirs),
        Cmd::Ide(op) => cmd::integration::ide(op, &dirs),
        Cmd::Watch => cmd::tooling::watch(&dirs),
        Cmd::Doctor => cmd::tooling::doctor(&dirs, cli.format),
        Cmd::Completions { shell } => cmd::tooling::completions(shell),
        Cmd::Man { dir } => cmd::tooling::man(dir),
        Cmd::Install { prefix } => cmd::tooling::install(prefix),
        Cmd::Explain { code } => cmd::tooling::explain(code),
    }
}

/// Honesty marker: local runtime success must never read as
/// native Windows success.
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
