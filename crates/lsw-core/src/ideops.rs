use std::path::PathBuf;

use serde::Serialize;

use lsw_config::TargetArch;

use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeEnv {
    pub target: String,
    pub environment: String,
    pub compiler: String,
    pub cxx_compiler: String,
    pub sysroot: String,
    pub include_paths: Vec<String>,
    pub defines: Vec<String>,
    pub c_flags: Vec<String>,
    pub cxx_flags: Vec<String>,
    pub link_flags: Vec<String>,
    pub wine_prefix: String,
    pub project_windows_root: Option<String>,
}

fn clang_triple(arch: TargetArch) -> &'static str {
    match arch {
        TargetArch::X86_64 => "x86_64-pc-windows-gnu",
        TargetArch::X86 => "i686-pc-windows-gnu",
        TargetArch::Aarch64 => "aarch64-pc-windows-gnu",
        TargetArch::Armv7 => "armv7-pc-windows-gnu",
        TargetArch::Arm64Ec => "arm64ec-pc-windows-gnu",
    }
}

pub fn ide_env(env: &Environment, project: Option<&Project>) -> Result<IdeEnv> {
    let tc = &env.manifest.toolchain;

    if !tc.cc.is_file() {
        return Err(Error::ToolMissing {
            tool: tc.cc.display().to_string(),
            fix: "recreate the environment: lsw env create <name> --force".into(),
        });
    }
    if !tc.sysroot.is_dir() {
        return Err(Error::ToolMissing {
            tool: tc.sysroot.display().to_string(),
            fix: "reinstall the mingw-w64 sysroot or recreate the environment".into(),
        });
    }

    let mut include_paths = vec![tc.sysroot.join("include").display().to_string()];
    let from_flags: Vec<String> = tc
        .cxx_flags
        .iter()
        .filter_map(|f| f.strip_prefix("-I").map(str::to_owned))
        .collect();
    if from_flags.is_empty() {
        include_paths.extend(libstdcxx_dirs(env));
    } else {
        include_paths.extend(from_flags);
    }

    Ok(IdeEnv {
        target: clang_triple(env.manifest.target_arch).to_owned(),
        environment: env.name.clone(),
        compiler: tc.cc.display().to_string(),
        cxx_compiler: tc.cxx.display().to_string(),
        sysroot: tc.sysroot.display().to_string(),
        include_paths,
        defines: vec!["_WIN32".to_owned()],
        c_flags: tc.c_flags.clone(),
        cxx_flags: tc.cxx_flags.clone(),
        link_flags: tc.link_flags.clone(),
        wine_prefix: env.layout.prefix().display().to_string(),
        project_windows_root: project.map(|p| format!("C:\\src\\{}", p.manifest.project.name)),
    })
}

fn libstdcxx_dirs(env: &Environment) -> Vec<String> {
    let triple = env.manifest.target_arch.mingw_triple();
    let base = env.manifest.toolchain.sysroot.join("include/c++");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("iostream").is_file())
        .collect();
    versions.sort();
    match versions.pop() {
        Some(dir) => {
            let target = dir.join(triple);
            let mut out = vec![dir.display().to_string()];
            if target.is_dir() {
                out.push(target.display().to_string());
            }
            out
        }
        None => Vec::new(),
    }
}
