mod crossfile;
mod error;
mod gnu;
mod msvc;
mod provider;
mod util;

pub use crossfile::*;
pub use error::*;
pub use gnu::*;
pub use msvc::*;
pub use provider::*;
pub use util::*;

#[cfg(test)]
mod tests {
    use super::*;
    use lsw_config::{ResolvedToolchain, TargetArch};
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn derive_sysroot_prefers_self_contained_then_falls_back_to_usr() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let bindir = root.join("bin");
        std::fs::create_dir_all(&bindir).unwrap();
        let inc = root.join("aarch64-w64-mingw32/include");
        std::fs::create_dir_all(&inc).unwrap();
        std::fs::write(inc.join("windows.h"), b"// h").unwrap();
        let cc = bindir.join("aarch64-w64-mingw32-gcc");
        std::fs::write(&cc, b"").unwrap();

        assert_eq!(
            derive_sysroot(&cc, "aarch64-w64-mingw32"),
            root.join("aarch64-w64-mingw32")
        );

        assert_eq!(
            derive_sysroot(
                std::path::Path::new("/usr/bin/x86_64-w64-mingw32-gcc"),
                "no-such-triple"
            ),
            PathBuf::from("/usr/no-such-triple")
        );
    }

    #[test]
    fn msvc_search_paths_reads_xwin_and_flat_layouts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for d in [
            "crt/include",
            "sdk/include/ucrt",
            "sdk/include/um",
            "sdk/include/shared",
            "crt/lib/x64",
            "sdk/lib/ucrt/x64",
            "sdk/lib/um/x64",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let (inc, lib) = msvc_search_paths(root, &["x64", "x86_64"]);
        assert!(inc.contains(&root.join("crt/include")));
        assert!(inc.contains(&root.join("sdk/include/ucrt")));
        assert!(inc.contains(&root.join("sdk/include/um")));
        assert!(lib.contains(&root.join("crt/lib/x64")));
        assert!(lib.contains(&root.join("sdk/lib/ucrt/x64")));
        assert!(lib.contains(&root.join("sdk/lib/um/x64")));
    }

    #[test]
    fn resolve_msvc_assembles_clang_cl_flags() {
        if which("clang-cl").is_none() || which("lld-link").is_none() {
            eprintln!("skipping: clang-cl/lld-link not installed");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("include")).unwrap();
        std::fs::create_dir_all(root.join("lib/x64")).unwrap();

        let tc = resolve_msvc(TargetArch::X86_64, root).unwrap();
        assert_eq!(tc.provider, CLANG_CL_ID);
        assert!(tc.cc.ends_with("clang-cl"));
        assert!(
            tc.c_flags
                .contains(&"--target=x86_64-pc-windows-msvc".to_owned())
        );
        let imsvc_pos = tc.c_flags.iter().position(|f| f == "-imsvc").unwrap();
        assert_eq!(
            tc.c_flags[imsvc_pos + 1],
            root.join("include").display().to_string()
        );
        assert!(tc.link_flags.contains(&"-fuse-ld=lld-link".to_owned()));
        assert!(
            tc.link_flags
                .iter()
                .any(|f| f.starts_with("/libpath:") && f.contains("lib/x64"))
        );
    }

    #[test]
    fn probe_msvc_compiles_headerless() {
        if which("clang-cl").is_none() || which("lld-link").is_none() {
            eprintln!("skipping: clang-cl/lld-link not installed");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("include")).unwrap();
        let tc = resolve_msvc(TargetArch::X86_64, tmp.path()).unwrap();
        let report = probe_msvc(&tc);
        assert!(report.compiled, "probe detail: {}", report.detail);
    }

    fn fake_toolchain() -> ResolvedToolchain {
        ResolvedToolchain {
            provider: LLVM_MINGW_ID.to_owned(),
            version: "clang 22.0.0".to_owned(),
            cc: PathBuf::from("/usr/bin/clang"),
            cxx: PathBuf::from("/usr/bin/clang++"),
            sysroot: PathBuf::from("/usr/x86_64-w64-mingw32"),
            c_flags: vec![
                "--target=x86_64-w64-mingw32".to_owned(),
                "--sysroot=/usr/x86_64-w64-mingw32".to_owned(),
            ],
            cxx_flags: vec!["-I/usr/x86_64-w64-mingw32/include/c++/16.1.0".to_owned()],
            link_flags: vec!["-fuse-ld=lld".to_owned()],
        }
    }

    fn skip(tool: &str) -> bool {
        if which(tool).is_none() {
            eprintln!("skipping: '{tool}' not on PATH");
            return true;
        }
        false
    }

    #[test]
    fn providers_prefers_llvm_mingw_first() {
        let ids: Vec<&str> = providers().iter().map(|p| p.id()).collect();
        assert_eq!(ids, vec![LLVM_MINGW_ID, MINGW_GCC_ID]);
    }

    #[test]
    fn cmake_file_contains_required_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toolchain.cmake");
        write_cmake_toolchain_file(&path, &fake_toolchain(), TargetArch::X86_64).unwrap();
        let text = fs::read_to_string(&path).unwrap();

        assert!(text.contains("set(CMAKE_SYSTEM_NAME Windows)"));
        assert!(text.contains("set(CMAKE_SYSTEM_PROCESSOR AMD64)"));
        assert!(text.contains("set(CMAKE_C_COMPILER \"/usr/bin/clang\")"));
        assert!(text.contains("set(CMAKE_CXX_COMPILER \"/usr/bin/clang++\")"));
        assert!(text.contains(
            "set(CMAKE_C_FLAGS_INIT \"--target=x86_64-w64-mingw32 --sysroot=/usr/x86_64-w64-mingw32\")"
        ));
        assert!(text.contains(
            "set(CMAKE_CXX_FLAGS_INIT \"--target=x86_64-w64-mingw32 --sysroot=/usr/x86_64-w64-mingw32 -I/usr/x86_64-w64-mingw32/include/c++/16.1.0\")"
        ));
        assert!(text.contains("set(CMAKE_EXE_LINKER_FLAGS_INIT \"-fuse-ld=lld\")"));
        assert!(text.contains("set(CMAKE_SHARED_LINKER_FLAGS_INIT \"-fuse-ld=lld\")"));
        assert!(text.contains("set(CMAKE_FIND_ROOT_PATH \"/usr/x86_64-w64-mingw32\")"));
        assert!(text.contains("set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)"));
        assert!(text.contains("set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)"));
        assert!(text.contains("set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)"));
        assert!(text.contains("set(CMAKE_EXECUTABLE_SUFFIX \".exe\")"));
    }

    #[test]
    fn cmake_file_windres_line_matches_host_availability() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("toolchain.cmake");
        write_cmake_toolchain_file(&path, &fake_toolchain(), TargetArch::X86_64).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let host_has_windres = which("x86_64-w64-mingw32-windres").is_some();
        assert_eq!(text.contains("CMAKE_RC_COMPILER"), host_has_windres);
    }

    #[test]
    fn cmake_processor_mapping_per_arch() {
        for (arch, processor) in [
            (TargetArch::X86_64, "AMD64"),
            (TargetArch::X86, "X86"),
            (TargetArch::Aarch64, "ARM64"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("toolchain.cmake");
            write_cmake_toolchain_file(&path, &fake_toolchain(), arch).unwrap();
            let text = fs::read_to_string(&path).unwrap();
            assert!(
                text.contains(&format!("set(CMAKE_SYSTEM_PROCESSOR {processor})")),
                "arch {arch} should map to {processor}"
            );
        }
    }

    #[test]
    fn sha256_of_known_bytes() {
        let dir = tempfile::tempdir().unwrap();

        let empty = dir.path().join("empty");
        fs::write(&empty, b"").unwrap();
        assert_eq!(
            sha256_file(&empty).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let abc = dir.path().join("abc");
        fs::write(&abc, b"abc").unwrap();
        assert_eq!(
            sha256_file(&abc).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_missing_file_is_io_error() {
        assert!(sha256_file(Path::new("/nonexistent/lsw-sha-test")).is_err());
    }

    #[test]
    fn compiler_version_unknown_on_missing_binary() {
        assert_eq!(
            compiler_version(Path::new("/nonexistent/lsw-cc-test")),
            "unknown"
        );
    }

    #[test]
    fn compiler_version_unknown_on_failing_binary() {
        if skip("false") {
            return;
        }
        assert_eq!(compiler_version(&which("false").unwrap()), "unknown");
    }

    #[test]
    fn compiler_version_first_line_of_real_compiler() {
        if skip("clang") {
            return;
        }
        let v = compiler_version(&which("clang").unwrap());
        assert_ne!(v, "unknown");
        assert!(!v.contains('\n'));
        assert!(v.to_lowercase().contains("clang"), "got: {v}");
    }

    #[test]
    fn which_finds_sh_and_rejects_garbage() {
        let sh = which("sh").expect("sh should exist on any unix host");
        assert!(sh.is_absolute());
        assert!(which("definitely-not-a-real-binary-lsw").is_none());
    }

    #[test]
    fn error_ids_are_stable() {
        let e = ToolchainError::ProviderUnavailable {
            id: "x".into(),
            detail: "d".into(),
        };
        assert!(e.to_string().starts_with("LSW1401"));
        let e = ToolchainError::ProbeFailed {
            id: "x".into(),
            detail: "d".into(),
        };
        assert!(e.to_string().starts_with("LSW1402"));
        let e = ToolchainError::UnknownProvider { id: "x".into() };
        assert!(e.to_string().starts_with("LSW1404"));
    }

    #[test]
    fn no_working_provider_lists_each_attempt_and_fixes() {
        let e = ToolchainError::NoWorkingProvider {
            attempts: vec![
                ("llvm-mingw".into(), "clang missing".into()),
                ("mingw-gcc".into(), "gcc missing".into()),
            ],
        };
        let msg = e.to_string();
        assert!(msg.starts_with("LSW1403"));
        assert!(msg.contains("  - llvm-mingw: clang missing\n"));
        assert!(msg.contains("  - mingw-gcc: gcc missing"));
        assert!(msg.contains("Possible fixes: install mingw-w64 toolchain or clang+lld"));
    }

    #[test]
    fn select_unknown_provider_errors() {
        let err = select(Some("totally-bogus"), TargetArch::X86_64).unwrap_err();
        assert!(matches!(err, ToolchainError::UnknownProvider { .. }));
        assert!(err.to_string().contains("totally-bogus"));
    }

    #[test]
    fn llvm_mingw_resolves_and_probes_when_available() {
        if skip("clang") || skip("clang++") {
            return;
        }
        let tc = match LlvmMingw.resolve(TargetArch::X86_64) {
            Ok(tc) => tc,
            Err(e) => {
                eprintln!("skipping: llvm-mingw sysroot unavailable: {e}");
                return;
            }
        };
        assert_eq!(tc.provider, LLVM_MINGW_ID);
        assert!(tc.cc.is_absolute());
        assert!(tc.cxx.is_absolute());
        assert!(tc.c_flags.iter().any(|f| f.starts_with("--target=")));
        assert!(tc.c_flags.iter().any(|f| f.starts_with("--sysroot=")));
        assert_eq!(
            tc.link_flags.first().map(String::as_str),
            Some("-fuse-ld=lld")
        );
        assert!(tc.link_flags.iter().any(|f| f == "-static"));
        assert!(tc.link_flags.iter().any(|f| f == "-lwinpthread"));
        for extra in &tc.link_flags[1..] {
            assert!(
                extra.starts_with("-L") || extra == "-static" || extra == "-lwinpthread",
                "unexpected link flag {extra}"
            );
        }

        let report = LlvmMingw.probe(TargetArch::X86_64).unwrap();
        assert_eq!(report.provider, LLVM_MINGW_ID);
        if !report.passed() {
            eprintln!(
                "note: llvm-mingw probe did not pass on this host: {}",
                report.detail
            );
        }
    }

    #[test]
    fn mingw_gcc_resolves_and_probe_produces_pe() {
        if skip("x86_64-w64-mingw32-gcc") || skip("x86_64-w64-mingw32-g++") {
            return;
        }
        let tc = MingwGcc.resolve(TargetArch::X86_64).unwrap();
        assert_eq!(tc.provider, MINGW_GCC_ID);
        assert!(tc.cc.is_absolute());
        assert!(tc.cc.ends_with("x86_64-w64-mingw32-gcc"));
        assert!(tc.c_flags.is_empty());
        assert_eq!(tc.link_flags, vec!["-static"]);
        assert_ne!(tc.version, "unknown");

        let report = MingwGcc.probe(TargetArch::X86_64).unwrap();
        assert!(report.compiled, "compile failed: {}", report.detail);
        assert!(report.linked, "link failed: {}", report.detail);
        assert!(report.produced_pe, "no PE produced: {}", report.detail);
    }

    #[test]
    fn select_auto_returns_working_toolchain() {
        let any_provider = which("clang").is_some() || which("x86_64-w64-mingw32-gcc").is_some();
        if !any_provider {
            eprintln!("skipping: neither clang nor x86_64-w64-mingw32-gcc on PATH");
            return;
        }
        match select(None, TargetArch::X86_64) {
            Ok((tc, report)) => {
                assert!(report.passed());
                assert_eq!(tc.provider, report.provider);
                assert!(tc.cc.is_absolute());
            }
            Err(ToolchainError::NoWorkingProvider { attempts }) => {
                assert_eq!(attempts.len(), 2);
                eprintln!("note: no provider passed on this host: {attempts:?}");
            }
            Err(e) => panic!("unexpected error variant: {e}"),
        }
    }

    #[test]
    fn select_preferred_mingw_gcc_when_available() {
        if skip("x86_64-w64-mingw32-gcc") || skip("x86_64-w64-mingw32-g++") {
            return;
        }
        let (tc, report) = select(Some(MINGW_GCC_ID), TargetArch::X86_64).unwrap();
        assert_eq!(tc.provider, MINGW_GCC_ID);
        assert!(report.passed());
    }

    #[test]
    fn probe_reports_failure_detail_for_broken_toolchain() {
        if skip("false") {
            return;
        }
        let cc = which("false").unwrap();
        let tc = ResolvedToolchain {
            provider: "broken".into(),
            version: "unknown".into(),
            cxx: cc.clone(),
            cc,
            sysroot: PathBuf::from("/nonexistent"),
            c_flags: vec![],
            cxx_flags: vec![],
            link_flags: vec![],
        };
        let report = run_probe("broken", &tc);
        assert!(!report.compiled);
        assert!(!report.linked);
        assert!(!report.produced_pe);
        assert!(report.detail.contains("compile failed"));
    }
}
