use std::path::PathBuf;

use lsw_core::{BuildOptions, Dirs, EnvCreateOptions, Project, Template};

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
        .unwrap_or(false)
}

fn tools_present() -> bool {
    on_path("wine") && on_path("cmake") && on_path("x86_64-w64-mingw32-gcc")
}

#[test]
fn e2e_init_build_inspect_audit() {
    if std::env::var("LSW_TEST_E2E").as_deref() != Ok("1") {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 to run");
        return;
    }
    if !tools_present() {
        eprintln!("skipping e2e: need wine, cmake, and x86_64-w64-mingw32-gcc on PATH");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        data: tmp.path().join("data"),
        config: tmp.path().join("config"),
        cache: tmp.path().join("cache"),
    };

    let init = lsw_core::init(tmp.path(), Some("e2e"), Template::Console).unwrap();
    let project = Project::discover(&init.root).unwrap();

    let env = lsw_core::env_create(
        &dirs,
        &EnvCreateOptions {
            name: "e2e".into(),
            arch: lsw_core::TargetArch::X86_64,
            toolchain: None,
            sdk: None,
            force: true,
            expose_home: false,
        },
    )
    .unwrap()
    .environment;

    let build = lsw_core::build(
        &project,
        &env,
        &BuildOptions {
            system: None,
            update_lock: true,
            reproducible: true,
            aot: false,
        },
    )
    .unwrap();
    assert!(!build.artifacts.is_empty(), "build produced no artifacts");

    let exe: PathBuf = build
        .artifacts
        .iter()
        .find(|a| a.extension().is_some_and(|e| e == "exe"))
        .map(|a| project.root.join(a))
        .expect("an .exe artifact");

    let inspect = lsw_core::inspect(&exe, Some(&env)).unwrap();
    assert_eq!(format!("{:?}", inspect.info.machine), "X86_64");
    assert!(!inspect.imports.is_empty());

    let audit = lsw_core::auditops::audit(&exe).unwrap();
    assert!(audit.hardened, "mingw output should have ASLR + DEP");

    assert_eq!(
        lsw_pe::coff_timestamp(&exe).unwrap(),
        0,
        "reproducible build"
    );
}
