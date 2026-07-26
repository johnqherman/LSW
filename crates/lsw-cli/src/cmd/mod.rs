use std::path::PathBuf;
use std::process::ExitCode;

pub(crate) fn emit_json(value: &impl serde::Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serializes")
    );
}

pub(crate) fn exit_ok(ok: bool) -> lsw_core::Result<ExitCode> {
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

pub(crate) enum Picked {
    None,
    One(PathBuf),
    Many(Vec<PathBuf>),
}

pub(crate) fn pick_built(build: &lsw_core::BuildReport, exe_only: bool) -> Picked {
    let mut hits: Vec<PathBuf> = build
        .artifacts
        .iter()
        .filter(|a| {
            a.extension().is_some_and(|e| {
                e.eq_ignore_ascii_case("exe") || (!exe_only && e.eq_ignore_ascii_case("dll"))
            })
        })
        .cloned()
        .collect();
    hits.sort();
    match hits.len() {
        0 => Picked::None,
        1 => Picked::One(hits.remove(0)),
        _ => Picked::Many(hits),
    }
}

pub(crate) fn resolve_pe(
    file: &Option<PathBuf>,
    dirs: &lsw_core::Dirs,
) -> lsw_core::Result<Option<PathBuf>> {
    if let Some(f) = file {
        return Ok(Some(f.clone()));
    }
    let (p, env) = crate::active_env(dirs)?;
    let build = lsw_core::build(&p, &env, &lsw_core::BuildOptions::default())?;
    match pick_built(&build, false) {
        Picked::None => {
            eprintln!("the build produced no .exe or .dll; pass a file explicitly");
            Ok(None)
        }
        Picked::One(only) => {
            let only = p.root.join(only);
            eprintln!("[lsw] using {}", only.display());
            Ok(Some(only))
        }
        Picked::Many(many) => {
            if let Picked::One(exe) = pick_built(&build, true) {
                let exe = p.root.join(exe);
                eprintln!("[lsw] using {}", exe.display());
                return Ok(Some(exe));
            }
            eprintln!("the build produced multiple artifacts; pick one:");
            for a in &many {
                eprintln!("  {}", p.root.join(a).display());
            }
            Ok(None)
        }
    }
}

pub(crate) mod build;
pub(crate) mod config;
pub(crate) mod debug;
pub(crate) mod inspect;
pub(crate) mod integration;
pub(crate) mod lang;
pub(crate) mod package;
pub(crate) mod project;
pub(crate) mod state;
pub(crate) mod tooling;
pub(crate) mod verify;
