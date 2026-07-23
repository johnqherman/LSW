use std::path::{Path, PathBuf};
use std::process::Command;

use lsw_core::{BuildOptions, Dirs, Display, Domain, EnvCreateOptions, Project, Sandbox, Template};

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
        .unwrap_or(false)
}

fn e2e_enabled() -> bool {
    std::env::var("LSW_TEST_E2E").as_deref() == Ok("1")
}

fn base_tools_present() -> bool {
    on_path("wine") && on_path("cmake") && on_path("x86_64-w64-mingw32-gcc")
}

fn dotnet_sdk_present() -> bool {
    on_path("dotnet")
        && Command::new("dotnet")
            .arg("--list-sdks")
            .output()
            .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

fn rust_windows_target_present() -> bool {
    on_path("cargo")
        && Command::new("rustc")
            .args(["--target", "x86_64-pc-windows-gnu", "--print", "cfg"])
            .output()
            .is_ok_and(|o| o.status.success())
}

struct Corpus {
    _tmp: tempfile::TempDir,
    dirs: Dirs,
    root: PathBuf,
}

fn corpus() -> Corpus {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        data: tmp.path().join("data"),
        config: tmp.path().join("config"),
        cache: tmp.path().join("cache"),
    };
    let root = tmp.path().to_path_buf();
    Corpus {
        _tmp: tmp,
        dirs,
        root,
    }
}

fn create_env(c: &Corpus, name: &str) -> lsw_core::envops::Environment {
    lsw_core::env_create(
        &c.dirs,
        &EnvCreateOptions {
            name: name.into(),
            arch: lsw_core::TargetArch::X86_64,
            toolchain: None,
            sdk: None,
            force: true,
            expose_home: false,
        },
    )
    .unwrap()
    .environment
}

fn build_opts() -> BuildOptions {
    BuildOptions {
        system: None,
        update_lock: true,
        reproducible: false,
        aot: false,
    }
}

fn find_artifact<'a>(
    artifacts: &'a [PathBuf],
    ext: &str,
    contains: Option<&str>,
) -> Option<&'a PathBuf> {
    artifacts.iter().find(|a| {
        a.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
            && contains.is_none_or(|c| a.to_string_lossy().contains(c))
    })
}

fn find_published_exe<'a>(artifacts: &'a [PathBuf], name: &str) -> Option<&'a PathBuf> {
    artifacts.iter().find(|a| {
        a.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            && a.file_stem().is_some_and(|s| s == name)
            && a.to_string_lossy().contains("publish")
    })
}

fn run_in_wine(env: &lsw_core::envops::Environment, project: &Project, exe: &Path) {
    let report = lsw_core::run(
        env,
        Some(project),
        exe,
        &[],
        Domain::Windows,
        Sandbox::None,
        Display::Inherit,
    )
    .unwrap();
    assert!(report.status.success(), "{} failed in wine", exe.display());
}

#[test]
fn e2e_console_cmake_build_run_inspect_audit() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    let c = corpus();
    let init = lsw_core::init(&c.root, Some("console"), Template::Console).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "console");

    let build = lsw_core::build(
        &project,
        &env,
        &BuildOptions {
            reproducible: true,
            ..build_opts()
        },
    )
    .unwrap();
    let exe = project
        .root
        .join(find_artifact(&build.artifacts, "exe", None).expect("an .exe artifact"));

    let inspect = lsw_core::inspect(&exe, Some(&env)).unwrap();
    assert_eq!(format!("{:?}", inspect.info.machine), "X86_64");
    assert!(!inspect.imports.is_empty());
    assert_eq!(format!("{:?}", inspect.info.subsystem), "Console");

    let audit = lsw_core::auditops::audit(&exe).unwrap();
    assert!(audit.hardened, "mingw output should have ASLR + DEP");
    assert_eq!(lsw_pe::coff_timestamp(&exe).unwrap(), 0, "reproducible");

    run_in_wine(&env, &project, &exe);
}

#[test]
fn e2e_gui_template_builds_windows_subsystem() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    let c = corpus();
    let init = lsw_core::init(&c.root, Some("gui"), Template::Gui).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "gui");

    let build = lsw_core::build(&project, &env, &build_opts()).unwrap();
    let exe = project
        .root
        .join(find_artifact(&build.artifacts, "exe", None).expect("an .exe artifact"));
    let inspect = lsw_core::inspect(&exe, Some(&env)).unwrap();
    assert_eq!(format!("{:?}", inspect.info.subsystem), "Gui");
}

#[test]
fn e2e_dll_template_builds_shared_library() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    let c = corpus();
    let init = lsw_core::init(&c.root, Some("shared"), Template::Dll).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "shared");

    let build = lsw_core::build(&project, &env, &build_opts()).unwrap();
    assert!(
        find_artifact(&build.artifacts, "dll", None).is_some(),
        "dll template must produce a .dll"
    );
}

#[test]
fn e2e_rust_cargo_builds_and_runs() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    if !rust_windows_target_present() {
        eprintln!("skipping e2e: rustc lacks the x86_64-pc-windows-gnu target");
        return;
    }
    let c = corpus();
    let init = lsw_core::rustops::init(&c.root, Some("rusty")).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "rusty");

    let build = lsw_core::build(&project, &env, &build_opts()).unwrap();
    let exe = project
        .root
        .join(find_artifact(&build.artifacts, "exe", None).expect("an .exe artifact"));
    run_in_wine(&env, &project, &exe);
}

#[test]
fn e2e_dotnet_managed_builds_and_runs() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    if !dotnet_sdk_present() {
        eprintln!("skipping e2e: no .NET SDK on PATH");
        return;
    }
    let c = corpus();
    let init = lsw_core::dotnetops::init(&c.root, Some("managed")).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "managed");

    let build = lsw_core::build(&project, &env, &build_opts()).unwrap();
    let exe = project
        .root
        .join(find_published_exe(&build.artifacts, "managed").expect("a published .exe"));
    run_in_wine(&env, &project, &exe);
}

#[test]
fn e2e_dotnet_aot_builds_native_pe_and_runs() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    if !dotnet_sdk_present() || !on_path("clang") || !on_path("lld-link") {
        eprintln!("skipping e2e: NativeAOT needs dotnet, clang, and lld-link");
        return;
    }
    let c = corpus();
    let init = lsw_core::dotnetops::init(&c.root, Some("aotapp")).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "aotapp");

    let build = lsw_core::build(
        &project,
        &env,
        &BuildOptions {
            aot: true,
            ..build_opts()
        },
    )
    .unwrap();
    let exe = project
        .root
        .join(find_published_exe(&build.artifacts, "aotapp").expect("a published .exe"));

    let inspect = lsw_core::inspect(&exe, Some(&env)).unwrap();
    let imported_dlls: Vec<String> = inspect
        .imports
        .iter()
        .map(|i| format!("{i:?}").to_lowercase())
        .collect();
    assert!(
        !imported_dlls.iter().any(|d| d.contains("hostfxr")),
        "AOT output must not depend on the managed host"
    );
    run_in_wine(&env, &project, &exe);
}

#[test]
fn e2e_msi_package_and_install_verify() {
    if !e2e_enabled() || !base_tools_present() {
        eprintln!("skipping e2e: set LSW_TEST_E2E=1 with wine/cmake/mingw on PATH");
        return;
    }
    if !on_path("wixl") {
        eprintln!("skipping e2e: wixl (msitools) not on PATH");
        return;
    }
    let c = corpus();
    let init = lsw_core::init(&c.root, Some("installer"), Template::Console).unwrap();
    let project = Project::discover(&init.root).unwrap();
    let env = create_env(&c, "installer");

    let report =
        lsw_core::packageops::package(&project, &env, lsw_core::packageops::PackageTarget::Msi)
            .unwrap();
    let msi = report.msi.expect("an .msi artifact");

    let verify = lsw_core::installops::verify_msi(
        &c.dirs,
        &env,
        &project.manifest.project.name,
        &msi,
        &report.files,
    )
    .unwrap();
    assert!(verify.uninstall_clean);
    assert!(!verify.files.is_empty());
}
