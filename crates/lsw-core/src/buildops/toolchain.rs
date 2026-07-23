use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lsw_config::{LinkMode, ResolvedToolchain, TargetArch};

use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

fn win32_winnt(api: &str) -> Option<(&'static str, &'static str)> {
    let v = match api.to_ascii_lowercase().as_str() {
        "winxp" | "xp" => ("0x0501", "0x05010000"),
        "vista" | "winvista" => ("0x0600", "0x06000000"),
        "win7" | "windows7" => ("0x0601", "0x06010000"),
        "win8" | "windows8" => ("0x0602", "0x06020000"),
        "win8.1" | "win81" => ("0x0603", "0x06030000"),
        "win10" | "windows10" => ("0x0A00", "0x0A000000"),
        "win11" | "windows11" => ("0x0A00", "0x0A00000C"),
        _ => return None,
    };
    Some(v)
}

pub(crate) fn api_defines(api: &str) -> Vec<String> {
    match win32_winnt(api) {
        Some((winnt, ntddi)) => vec![
            format!("-D_WIN32_WINNT={winnt}"),
            format!("-DWINVER={winnt}"),
            format!("-DNTDDI_VERSION={ntddi}"),
        ],
        None => Vec::new(),
    }
}

fn meson_cpu_family(arch: TargetArch) -> &'static str {
    match arch {
        TargetArch::X86_64 => "x86_64",
        TargetArch::X86 => "x86",
        TargetArch::Aarch64 | TargetArch::Arm64Ec => "aarch64",
        TargetArch::Armv7 => "arm",
    }
}

pub(crate) fn write_meson_cross_file(
    path: &Path,
    tc: &ResolvedToolchain,
    arch: TargetArch,
) -> std::io::Result<()> {
    let triple = arch.mingw_triple();
    let family = meson_cpu_family(arch);
    let text = format!(
        "[binaries]\nc = '{cc}'\ncpp = '{cxx}'\nar = '{triple}-ar'\nstrip = '{triple}-strip'\nwindres = '{triple}-windres'\n\n[host_machine]\nsystem = 'windows'\ncpu_family = '{family}'\ncpu = '{family}'\nendian = 'little'\n",
        cc = tc.cc.display(),
        cxx = tc.cxx.display(),
    );
    fs::write(path, text)
}

pub(crate) fn effective_toolchain(env: &Environment, project: &Project) -> ResolvedToolchain {
    let mut tc = env.manifest.toolchain.clone();
    if project.manifest.toolchain.link == LinkMode::Dynamic {
        tc.link_flags
            .retain(|f| f != "-static" && f != "-lwinpthread");
    }
    if let Some(api) = &project.manifest.target.api {
        tc.c_flags.extend(api_defines(api));
    }
    tc
}

pub(crate) fn run_step(
    project: &Project,
    env: &Environment,
    tc: &ResolvedToolchain,
    argv: &[String],
    commands: &mut Vec<String>,
) -> Result<()> {
    let (program, args) = argv.split_first().ok_or(Error::NoBuildSystem)?;
    let rendered = argv.join(" ");
    commands.push(rendered.clone());

    let mut c_flags = tc.c_flags.join(" ");
    let mut cxx_flags = tc
        .c_flags
        .iter()
        .chain(&tc.cxx_flags)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let mut link_flags = tc.link_flags.join(" ");
    if let Some((include, lib, _bin)) = crate::depsops::dep_dirs(project) {
        let include_flag = format!(" -I{}", include.display());
        c_flags.push_str(&include_flag);
        cxx_flags.push_str(&include_flag);
        link_flags.push_str(&format!(" -L{}", lib.display()));
    }
    let mut command = Command::new(program);
    lsw_runtime::scrub_wine_env(&mut command);
    command
        .args(args)
        .current_dir(&project.root)
        .stdout(crate::diagnostic_stdio())
        .env("WINEPREFIX", env.layout.prefix())
        .env("CC", &tc.cc)
        .env("CXX", &tc.cxx)
        .env("CFLAGS", &c_flags)
        .env("CXXFLAGS", &cxx_flags)
        .env("LDFLAGS", &link_flags)
        .env("LSW_ENV", &env.name)
        .env("LSW_TARGET_FLAGS", &c_flags);
    let status = command.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing {
                tool: program.clone(),
                fix: format!("install {program} or adjust [build].command in lsw.toml"),
            }
        } else {
            Error::io(PathBuf::from(program), e)
        }
    })?;

    if !status.success() {
        return Err(Error::BuildFailed {
            command: rendered,
            code: status.code(),
        });
    }
    Ok(())
}
