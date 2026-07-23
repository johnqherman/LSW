use std::fs;
use std::path::{Path, PathBuf};

use lsw_config::{ResolvedToolchain, TargetArch};

use crate::envops::{self, Environment};
use crate::error::{Error, Result};
use crate::project::Project;

pub mod aot;
mod lockfile;
mod toolchain;

pub(crate) use lockfile::check_lock;

use lockfile::{stamp_build_dir, sync_lockfile};
use toolchain::{effective_toolchain, run_step, write_meson_cross_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildSystem {
    Cmake,
    Cargo,
    Make,
    Ninja,
    Meson,
    Zig,
    Dotnet,
    Explicit,
}

fn has_dotnet_project(root: &Path) -> bool {
    std::fs::read_dir(root)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.ends_with(".csproj") || name.ends_with(".sln") || name.ends_with(".fsproj")
            })
        })
        .unwrap_or(false)
}

fn check_case_sensitivity(project: &Project) -> Result<()> {
    let hazards = crate::caseops::hazards(&project.root);
    if hazards.is_empty() {
        return Ok(());
    }
    let detail = hazards
        .iter()
        .map(|h| format!("  {}: {}", h.dir, h.names.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    if project.manifest.filesystem.case == lsw_config::CaseSensitivity::Strict {
        return Err(Error::CaseCollision { detail });
    }
    for h in &hazards {
        tracing::warn!(
            dir = %h.dir,
            names = %h.names.join(", "),
            "case-insensitive filename collision: these clash on Windows"
        );
    }
    Ok(())
}

pub(crate) fn detect_build_system(root: &Path) -> Option<BuildSystem> {
    if root.join("CMakeLists.txt").is_file() {
        Some(BuildSystem::Cmake)
    } else if root.join("meson.build").is_file() {
        Some(BuildSystem::Meson)
    } else if root.join("build.zig").is_file() {
        Some(BuildSystem::Zig)
    } else if root.join("Cargo.toml").is_file() {
        Some(BuildSystem::Cargo)
    } else if has_dotnet_project(root) {
        Some(BuildSystem::Dotnet)
    } else if root.join("build.ninja").is_file() {
        Some(BuildSystem::Ninja)
    } else if root.join("Makefile").is_file() || root.join("makefile").is_file() {
        Some(BuildSystem::Make)
    } else {
        None
    }
}

fn build_system_from_name(name: &str) -> Option<BuildSystem> {
    match name {
        "cmake" => Some(BuildSystem::Cmake),
        "cargo" => Some(BuildSystem::Cargo),
        "make" => Some(BuildSystem::Make),
        "ninja" => Some(BuildSystem::Ninja),
        "meson" => Some(BuildSystem::Meson),
        "zig" => Some(BuildSystem::Zig),
        "dotnet" => Some(BuildSystem::Dotnet),
        _ => None,
    }
}

fn zig_target(arch: TargetArch) -> Option<&'static str> {
    match arch {
        TargetArch::X86_64 => Some("x86_64-windows-gnu"),
        TargetArch::X86 => Some("x86-windows-gnu"),
        TargetArch::Aarch64 => Some("aarch64-windows-gnu"),
        TargetArch::Armv7 => Some("arm-windows-gnu"),
        TargetArch::Arm64Ec => None,
    }
}

fn dotnet_rid(arch: TargetArch) -> Option<&'static str> {
    match arch {
        TargetArch::X86_64 => Some("win-x64"),
        TargetArch::X86 => Some("win-x86"),
        TargetArch::Aarch64 | TargetArch::Arm64Ec => Some("win-arm64"),
        TargetArch::Armv7 => None,
    }
}

#[derive(Debug, Default)]
pub struct BuildOptions {
    pub system: Option<String>,
    pub update_lock: bool,
    pub reproducible: bool,
    pub aot: bool,
}

#[derive(Debug)]
pub struct BuildReport {
    pub system: BuildSystem,
    pub commands: Vec<String>,
    pub artifacts: Vec<PathBuf>,
    pub lock_written: bool,
}

pub fn build(project: &Project, env: &Environment, opts: &BuildOptions) -> Result<BuildReport> {
    check_case_sensitivity(project)?;
    envops::link_project(env, project)?;
    let lock_written = sync_lockfile(project, env, opts.update_lock)?;
    stamp_build_dir(project, env)?;

    let explicit = project.manifest.build.as_ref();
    let system = match (opts.system.as_deref(), explicit) {
        (Some(name), _) if build_system_from_name(name).is_some() => {
            build_system_from_name(name).unwrap()
        }
        (Some(_), Some(_)) | (None, Some(_)) => BuildSystem::Explicit,
        (Some(_), None) | (None, None) => {
            detect_build_system(&project.root).ok_or(Error::NoBuildSystem)?
        }
    };

    let mut tc = effective_toolchain(env, project);
    if opts.reproducible {
        tc.link_flags.push("-Wl,--no-insert-timestamp".to_owned());
    }
    let mut commands = Vec::new();
    let mut artifact_dir = project.root.join("build");
    match system {
        BuildSystem::Explicit => {
            let spec = explicit.ok_or(Error::NoBuildSystem)?;
            run_step(project, env, &tc, &spec.command, &mut commands)?;
        }
        BuildSystem::Cargo => {
            let triple = env.manifest.target_arch.rust_gnu_triple().ok_or_else(|| {
                Error::RustTargetUnavailable {
                    arch: env.manifest.target_arch.to_string(),
                }
            })?;
            crate::rustops::ensure_target(env.manifest.target_arch)?;
            run_step(
                project,
                env,
                &tc,
                &[
                    "cargo".to_owned(),
                    "build".to_owned(),
                    "--target".to_owned(),
                    triple.to_owned(),
                ],
                &mut commands,
            )?;
            artifact_dir = project.root.join("target").join(triple).join("debug");
        }
        BuildSystem::Cmake => {
            let toolchain_file = env.layout.cmake_toolchain_file();
            lsw_toolchain::write_cmake_toolchain_file(
                &toolchain_file,
                &tc,
                env.manifest.target_arch,
            )
            .map_err(|e| Error::io(toolchain_file.clone(), e))?;
            refresh_stale_cmake_build_dir(&project.root.join("build"), &tc)?;

            let generator = if which("ninja").is_some() {
                Some("Ninja")
            } else {
                None
            };
            let mut configure = vec![
                "cmake".to_owned(),
                "-S".to_owned(),
                ".".to_owned(),
                "-B".to_owned(),
                "build".to_owned(),
                format!("-DCMAKE_TOOLCHAIN_FILE={}", toolchain_file.display()),
                "-DCMAKE_BUILD_TYPE=Debug".to_owned(),
                format!(
                    "-DCMAKE_CROSSCOMPILING_EMULATOR={}",
                    env.manifest.runtime.executable.display()
                ),
            ];
            if let Some(g) = generator {
                configure.push("-G".to_owned());
                configure.push(g.to_owned());
            }
            run_step(project, env, &tc, &configure, &mut commands)?;
            run_step(
                project,
                env,
                &tc,
                &["cmake".to_owned(), "--build".to_owned(), "build".to_owned()],
                &mut commands,
            )?;
            write_cmake_toolchain_marker(&project.root.join("build"), &tc);
        }
        BuildSystem::Make => {
            run_step(project, env, &tc, &["make".to_owned()], &mut commands)?;
            artifact_dir = project.root.clone();
        }
        BuildSystem::Ninja => {
            run_step(project, env, &tc, &["ninja".to_owned()], &mut commands)?;
            artifact_dir = project.root.clone();
        }
        BuildSystem::Zig => {
            let target = zig_target(env.manifest.target_arch).ok_or_else(|| {
                Error::RustTargetUnavailable {
                    arch: env.manifest.target_arch.to_string(),
                }
            })?;
            run_step(
                project,
                env,
                &tc,
                &[
                    "zig".to_owned(),
                    "build".to_owned(),
                    format!("-Dtarget={target}"),
                ],
                &mut commands,
            )?;
            artifact_dir = project.root.join("zig-out");
        }
        BuildSystem::Dotnet => {
            let rid = dotnet_rid(env.manifest.target_arch).ok_or_else(|| {
                Error::RustTargetUnavailable {
                    arch: env.manifest.target_arch.to_string(),
                }
            })?;
            let mut args = vec![
                "dotnet".to_owned(),
                "publish".to_owned(),
                "-c".to_owned(),
                "Debug".to_owned(),
                "-r".to_owned(),
                rid.to_owned(),
                "--self-contained".to_owned(),
                "true".to_owned(),
            ];
            if opts.aot || project.manifest.toolchain.aot {
                let setup = aot::prepare(project, env, &tc)?;
                args.extend(aot::publish_args(&setup));
            }
            run_step(project, env, &tc, &args, &mut commands)?;
            artifact_dir = project.root.join("bin");
        }
        BuildSystem::Meson => {
            let cross_file = env.layout.root.join("meson-cross.ini");
            write_meson_cross_file(&cross_file, &tc, env.manifest.target_arch)
                .map_err(|e| Error::io(cross_file.clone(), e))?;
            if !project.root.join("build").join("meson-info").is_dir() {
                run_step(
                    project,
                    env,
                    &tc,
                    &[
                        "meson".to_owned(),
                        "setup".to_owned(),
                        "build".to_owned(),
                        format!("--cross-file={}", cross_file.display()),
                    ],
                    &mut commands,
                )?;
            }
            run_step(
                project,
                env,
                &tc,
                &[
                    "meson".to_owned(),
                    "compile".to_owned(),
                    "-C".to_owned(),
                    "build".to_owned(),
                ],
                &mut commands,
            )?;
        }
    }

    let mut artifacts = find_artifacts(&artifact_dir, &project.root);
    verify_artifacts_are_pe(project, &artifacts)?;

    if opts.reproducible {
        for artifact in &artifacts {
            let _ = lsw_pe::set_coff_timestamp(&project.root.join(artifact), 0);
        }
    }

    let deployed = deploy_runtime_dlls(&tc, &artifacts, &project.root)?;
    if !deployed.is_empty() {
        artifacts.extend(deployed);
        artifacts.sort();
        artifacts.dedup();
    }

    Ok(BuildReport {
        system,
        commands,
        artifacts,
        lock_written,
    })
}

fn verify_artifacts_are_pe(project: &Project, artifacts: &[PathBuf]) -> Result<()> {
    for artifact in artifacts {
        let absolute = project.root.join(artifact);
        match lsw_pe::detect(&absolute)? {
            lsw_pe::BinaryKind::Pe(_) => {}
            other => {
                return Err(Error::ArtifactNotPe {
                    artifact: artifact.clone(),
                    found: format!("{other:?}"),
                });
            }
        }
    }
    Ok(())
}

fn refresh_stale_cmake_build_dir(
    build_dir: &Path,
    tc: &lsw_config::ResolvedToolchain,
) -> Result<()> {
    use sha2::Digest;
    if !build_dir.join("CMakeCache.txt").is_file() {
        return Ok(());
    }
    let fingerprint = format!("{:x}", sha2::Sha256::digest(format!("{tc:?}").as_bytes()));
    let marker = build_dir.join(".lsw-toolchain");
    if fs::read_to_string(&marker).is_ok_and(|m| m.trim() == fingerprint) {
        return Ok(());
    }
    fs::remove_dir_all(build_dir).map_err(|e| Error::io(build_dir.to_path_buf(), e))?;
    fs::create_dir_all(build_dir).map_err(|e| Error::io(build_dir.to_path_buf(), e))?;
    fs::write(&marker, fingerprint).map_err(|e| Error::io(marker.clone(), e))?;
    Ok(())
}

fn write_cmake_toolchain_marker(build_dir: &Path, tc: &lsw_config::ResolvedToolchain) {
    use sha2::Digest;
    let fingerprint = format!("{:x}", sha2::Sha256::digest(format!("{tc:?}").as_bytes()));
    let _ = fs::create_dir_all(build_dir);
    let _ = fs::write(build_dir.join(".lsw-toolchain"), fingerprint);
}

fn find_artifacts(build_dir: &std::path::Path, project_root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(build_dir, &mut out);
    out.sort();
    out.into_iter()
        .map(|p| p.strip_prefix(project_root).map(PathBuf::from).unwrap_or(p))
        .collect()
}

fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let cargo_internal = (name == "deps" || name == "build") && dir.join("deps").is_dir();
            if name == "CMakeFiles" || cargo_internal {
                continue;
            }
            walk(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
        {
            out.push(path);
        }
    }
}

pub(crate) fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(program))
        .find(|c| c.is_file())
}

fn deploy_runtime_dlls(
    tc: &ResolvedToolchain,
    artifacts: &[PathBuf],
    project_root: &Path,
) -> Result<Vec<PathBuf>> {
    let sysroot_bin = tc.sysroot.join("bin");
    if !sysroot_bin.is_dir() {
        return Ok(Vec::new());
    }
    let mut available: std::collections::BTreeMap<String, PathBuf> =
        std::collections::BTreeMap::new();
    for entry in fs::read_dir(&sysroot_bin)
        .map_err(|e| Error::io(sysroot_bin.clone(), e))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase().ends_with(".dll") {
            available.insert(name.to_ascii_lowercase(), entry.path());
        }
    }
    if available.is_empty() {
        return Ok(Vec::new());
    }

    let mut deployed = Vec::new();
    let mut done: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    for artifact in artifacts {
        let abs = project_root.join(artifact);
        let Some(dir) = abs.parent().map(Path::to_path_buf) else {
            continue;
        };
        let mut work = vec![abs.clone()];
        while let Some(pe) = work.pop() {
            let Ok(imports) = lsw_pe::imports(&pe) else {
                continue;
            };
            for import in imports {
                let Some(src) = available.get(&import.to_ascii_lowercase()) else {
                    continue;
                };
                let target = dir.join(src.file_name().expect("dll has a file name"));
                if !done.insert(target.clone()) {
                    continue;
                }
                if !target.exists() {
                    fs::copy(src, &target).map_err(|e| Error::io(target.clone(), e))?;
                }
                if let Ok(rel) = target.strip_prefix(project_root) {
                    deployed.push(rel.to_path_buf());
                }
                work.push(target);
            }
        }
    }
    Ok(deployed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildops::toolchain::api_defines;
    use lsw_config::LinkMode;
    use std::process::Command;

    #[test]
    fn which_finds_sh() {
        assert!(which("sh").is_some());
        assert!(which("definitely-not-a-real-tool-xyz").is_none());
    }

    #[test]
    fn artifacts_walk_skips_cmakefiles() {
        let tmp = tempfile::tempdir().unwrap();
        let build = tmp.path().join("build");
        fs::create_dir_all(build.join("CMakeFiles/x")).unwrap();
        fs::write(build.join("app.exe"), b"MZ").unwrap();
        fs::write(build.join("CMakeFiles/x/probe.exe"), b"MZ").unwrap();
        fs::write(build.join("notes.txt"), b"x").unwrap();
        let found = find_artifacts(&build, tmp.path());
        assert_eq!(found, vec![PathBuf::from("build/app.exe")]);
    }

    fn tc_with(link_flags: Vec<String>, sysroot: PathBuf) -> ResolvedToolchain {
        ResolvedToolchain {
            provider: "mingw-gcc".into(),
            version: "test".into(),
            cc: PathBuf::from("/usr/bin/x86_64-w64-mingw32-gcc"),
            cxx: PathBuf::from("/usr/bin/x86_64-w64-mingw32-g++"),
            sysroot,
            c_flags: vec![],
            cxx_flags: vec![],
            link_flags,
        }
    }

    #[test]
    fn build_system_detection_prefers_in_a_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(detect_build_system(root), None);
        std::fs::write(root.join("Makefile"), "all:\n").unwrap();
        assert_eq!(detect_build_system(root), Some(BuildSystem::Make));
        std::fs::write(root.join("meson.build"), "project('x')\n").unwrap();
        assert_eq!(detect_build_system(root), Some(BuildSystem::Meson));
        std::fs::write(root.join("CMakeLists.txt"), "").unwrap();
        assert_eq!(detect_build_system(root), Some(BuildSystem::Cmake));
        assert_eq!(build_system_from_name("ninja"), Some(BuildSystem::Ninja));
        assert_eq!(build_system_from_name("bogus"), None);
    }

    #[test]
    fn api_defines_map_known_versions_and_ignore_unknown() {
        assert_eq!(
            api_defines("win10"),
            vec![
                "-D_WIN32_WINNT=0x0A00",
                "-DWINVER=0x0A00",
                "-DNTDDI_VERSION=0x0A000000",
            ]
        );
        assert_eq!(api_defines("win7")[0], "-D_WIN32_WINNT=0x0601");
        assert!(api_defines("nonsense").is_empty());
    }

    #[test]
    fn dynamic_link_strips_static_flags() {
        use lsw_config::{ProjectManifest, ProjectSection};
        let tmp = tempfile::tempdir().unwrap();
        let env = crate::envops::Environment {
            name: "e".into(),
            layout: lsw_config::EnvironmentLayout::new(tmp.path().join("env")),
            manifest: lsw_config::EnvironmentManifest {
                name: "e".into(),
                format: lsw_config::ENVIRONMENT_FORMAT_VERSION,
                target_arch: lsw_config::TargetArch::X86_64,
                toolchain: tc_with(
                    vec![
                        "-static".into(),
                        "-lwinpthread".into(),
                        "-fuse-ld=lld".into(),
                    ],
                    PathBuf::from("/usr/x86_64-w64-mingw32"),
                ),
                runtime: lsw_config::ResolvedRuntime {
                    provider: "wine".into(),
                    version: "11".into(),
                    executable: PathBuf::from("/usr/bin/wine"),
                },
            },
        };
        let mut manifest = ProjectManifest {
            project: ProjectSection { name: "p".into() },
            ..Default::default()
        };
        let project = Project {
            root: tmp.path().to_path_buf(),
            manifest: {
                manifest.toolchain.link = LinkMode::Dynamic;
                manifest
            },
        };
        let eff = effective_toolchain(&env, &project);
        assert!(!eff.link_flags.iter().any(|f| f == "-static"));
        assert!(!eff.link_flags.iter().any(|f| f == "-lwinpthread"));
        assert!(eff.link_flags.iter().any(|f| f == "-fuse-ld=lld"));
    }

    #[test]
    fn deploy_copies_imported_dlls_only_case_insensitive() {
        let cc = "x86_64-w64-mingw32-gcc";
        if which(cc).is_none() {
            eprintln!("skipping: {cc} not installed");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let sysroot = tmp.path().join("sysroot");
        fs::create_dir_all(sysroot.join("bin")).unwrap();
        fs::write(sysroot.join("bin/kernel32.dll"), b"MZ not-a-real-pe").unwrap();
        fs::write(
            sysroot.join("bin/libnotimported-1.dll"),
            b"MZ not-a-real-pe",
        )
        .unwrap();

        let build = tmp.path().join("build");
        fs::create_dir_all(&build).unwrap();
        let src = tmp.path().join("t.c");
        fs::write(&src, "int main(void){return 0;}\n").unwrap();
        let ok = Command::new(cc)
            .arg(&src)
            .arg("-o")
            .arg(build.join("t.exe"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("skipping: cross compile failed");
            return;
        }

        let tc = tc_with(vec![], sysroot);
        let deployed =
            deploy_runtime_dlls(&tc, &[PathBuf::from("build/t.exe")], tmp.path()).unwrap();
        assert!(build.join("kernel32.dll").is_file());
        assert!(!build.join("libnotimported-1.dll").is_file());
        assert!(deployed.contains(&PathBuf::from("build/kernel32.dll")));
    }
}
