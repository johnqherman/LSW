use serde::Serialize;

use crate::envops::Environment;
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

pub fn ide_env(env: &Environment, project: Option<&Project>) -> IdeEnv {
    let tc = &env.manifest.toolchain;
    let mut include_paths = vec![
        env.manifest
            .toolchain
            .sysroot
            .join("include")
            .display()
            .to_string(),
    ];
    include_paths.extend(
        tc.cxx_flags
            .iter()
            .filter_map(|f| f.strip_prefix("-I").map(str::to_owned)),
    );

    IdeEnv {
        target: format!("{}-pc-windows", env.manifest.target_arch),
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
    }
}
