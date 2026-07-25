use std::path::PathBuf;

pub(crate) fn resolve_pe(
    file: &Option<PathBuf>,
    dirs: &lsw_core::Dirs,
) -> lsw_core::Result<Option<PathBuf>> {
    if let Some(f) = file {
        return Ok(Some(f.clone()));
    }
    let (p, env) = crate::active_env(dirs)?;
    let build = lsw_core::build(&p, &env, &lsw_core::BuildOptions::default())?;
    let mut pes: Vec<PathBuf> = build
        .artifacts
        .iter()
        .filter(|a| {
            a.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
        })
        .map(|a| p.root.join(a))
        .collect();
    pes.sort();
    match pes.as_slice() {
        [] => {
            eprintln!("the build produced no .exe or .dll; pass a file explicitly");
            Ok(None)
        }
        [only] => {
            eprintln!("[lsw] using {}", only.display());
            Ok(Some(only.clone()))
        }
        many => {
            let exes: Vec<&PathBuf> = many
                .iter()
                .filter(|a| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
                .collect();
            if let [one_exe] = exes.as_slice() {
                eprintln!("[lsw] using {}", one_exe.display());
                return Ok(Some((*one_exe).clone()));
            }
            eprintln!("the build produced multiple artifacts; pick one:");
            for a in many {
                eprintln!("  {}", a.display());
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
