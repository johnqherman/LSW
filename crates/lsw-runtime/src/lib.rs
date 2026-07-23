mod env;
mod gpu;
mod sandbox;
mod types;
mod wine;

pub use env::*;
pub use sandbox::*;
pub use types::*;
pub use wine::*;

#[cfg(test)]
mod tests {
    use crate::sandbox::should_unshare_net;
    use crate::wine::{SYSTEM_REG, find_in_paths, find_wine, full_env, parse_wine_version};
    use crate::*;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    #[test]
    fn bwrap_args_lock_down_the_filesystem_and_namespaces() {
        let spec = SandboxSpec {
            rw_binds: vec![PathBuf::from("/data/env"), PathBuf::from("/home/u/proj")],
            network: NetworkMode::None,
            ..Default::default()
        };
        let args = bwrap_args(&spec, true);
        let ro_usr = args.windows(3).any(|w| w == ["--ro-bind", "/usr", "/usr"]);
        assert!(ro_usr, "must ro-bind /usr");
        for flag in [
            "--unshare-pid",
            "--unshare-uts",
            "--unshare-ipc",
            "--unshare-net",
        ] {
            assert!(args.iter().any(|a| a == flag), "missing {flag}");
        }
        let bind_env = args
            .windows(3)
            .position(|w| w == ["--bind", "/data/env", "/data/env"]);
        assert!(bind_env.is_some(), "env dir must be writable");
        assert!(
            args.windows(3)
                .any(|w| w == ["--bind", "/home/u/proj", "/home/u/proj"]),
            "project dir must be writable"
        );
    }

    #[test]
    fn bwrap_args_keep_network_when_requested() {
        let spec = SandboxSpec {
            rw_binds: vec![],
            network: NetworkMode::Host,
            ..Default::default()
        };
        assert!(
            !bwrap_args(&spec, false)
                .iter()
                .any(|a| a == "--unshare-net")
        );
    }

    #[test]
    fn network_mode_decides_unshare_net() {
        assert!(!should_unshare_net(NetworkMode::Host, false));
        assert!(should_unshare_net(NetworkMode::None, true));
        assert!(!should_unshare_net(NetworkMode::Isolated, true));
        assert!(should_unshare_net(NetworkMode::Isolated, false));
    }

    fn skip_without_wine(test: &str) -> bool {
        if find_wine().is_none() {
            eprintln!("skipping {test}: wine not found on PATH");
            return true;
        }
        false
    }

    fn skip_without_wine_gate(test: &str) -> bool {
        if std::env::var("LSW_TEST_WINE").as_deref() != Ok("1") {
            eprintln!("skipping {test}: set LSW_TEST_WINE=1 to run wine integration tests");
            return true;
        }
        skip_without_wine(test)
    }

    #[test]
    fn resolve_finds_wine_with_version_and_absolute_path() {
        if skip_without_wine("resolve_finds_wine_with_version_and_absolute_path") {
            return;
        }
        let resolved = WineRuntime.resolve().unwrap();
        assert_eq!(resolved.provider, "wine");
        assert!(!resolved.version.is_empty(), "version must be non-empty");
        assert!(
            !resolved.version.starts_with("wine-"),
            "'wine-' prefix must be stripped, got {}",
            resolved.version
        );
        assert!(resolved.executable.is_absolute());
        assert!(resolved.executable.is_file());
    }

    #[test]
    fn find_in_paths_misses_on_empty_path() {
        assert_eq!(find_in_paths("wine", OsStr::new("")), None);
    }

    #[test]
    fn find_in_paths_ignores_non_executables_and_finds_executables() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("wine");
        std::fs::write(&plain, "not executable").unwrap();
        let paths = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(
            find_in_paths("wine", &paths),
            None,
            "mode 644 must not match"
        );

        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = find_in_paths("wine", &paths).unwrap();
        assert!(found.is_absolute());
        assert_eq!(found.file_name().unwrap(), "wine");
    }

    #[test]
    fn wine_not_found_error_is_lsw1501_and_actionable() {
        let msg = RuntimeError::WineNotFound.to_string();
        assert!(msg.starts_with("LSW1501"), "got: {msg}");
        assert!(msg.contains("package manager"), "must suggest a fix: {msg}");
    }

    #[test]
    fn error_ids_are_stable() {
        let spawn = RuntimeError::SpawnFailed {
            program: PathBuf::from("/x/app.exe"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        };
        assert!(spawn.to_string().starts_with("LSW1503"));
        let init = RuntimeError::PrefixInitFailed {
            detail: "boom".into(),
        };
        assert!(init.to_string().starts_with("LSW1502"));
        assert!(init.to_string().contains("boom"));
        let exec = RuntimeError::ExecutionFailed {
            detail: "boom".into(),
        };
        assert!(exec.to_string().starts_with("LSW1504"));
    }

    #[test]
    fn parse_wine_version_strips_prefix() {
        assert_eq!(parse_wine_version("wine-11.12\n"), "11.12");
        assert_eq!(parse_wine_version("wine-10.0 (Staging)"), "10.0 (Staging)");
        assert_eq!(parse_wine_version("11.12"), "11.12");
        assert_eq!(parse_wine_version(""), "");
    }

    #[test]
    fn base_env_sets_prefix_debug_and_dll_overrides() {
        let env = base_env(Path::new("/data/lsw/environments/e1/prefix"));
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .unwrap_or_else(|| panic!("{key} missing"))
        };
        assert_eq!(lookup("WINEPREFIX"), "/data/lsw/environments/e1/prefix");
        assert_eq!(lookup("WINEDEBUG"), "fixme-all");
        assert_eq!(lookup("WINEDLLOVERRIDES"), "winemenubuilder.exe=d");
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn full_env_keeps_caller_entries_last_so_they_override() {
        let extra = vec![
            ("WINEDEBUG".to_owned(), "+loaddll".to_owned()),
            ("MY_VAR".to_owned(), "1".to_owned()),
        ];
        let env = full_env(Path::new("/p"), &extra);
        assert_eq!(&env[env.len() - extra.len()..], &extra[..]);
        let last_winedebug = env
            .iter()
            .rev()
            .find(|(k, _)| k == "WINEDEBUG")
            .map(|(_, v)| v.as_str());
        assert_eq!(last_winedebug, Some("+loaddll"));
    }

    #[test]
    fn egl_pin_only_on_all_nvidia_systems() {
        use crate::gpu::egl_vendor_pin_for;
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("10_nvidia.json");
        std::fs::write(&json, "{}").unwrap();
        let nv = |n: usize| vec!["0x10de".to_owned(); n];

        let pin = egl_vendor_pin_for(false, &nv(2), &json).unwrap();
        assert_eq!(pin.0, "__EGL_VENDOR_LIBRARY_FILENAMES");
        assert_eq!(pin.1, json.display().to_string());

        let hybrid = vec!["0x1002".to_owned(), "0x10de".to_owned()];
        assert!(egl_vendor_pin_for(false, &hybrid, &json).is_none());
        assert!(egl_vendor_pin_for(false, &["0x1002".to_owned()], &json).is_none());
        assert!(egl_vendor_pin_for(false, &[], &json).is_none());
        assert!(egl_vendor_pin_for(true, &nv(1), &json).is_none());
        assert!(egl_vendor_pin_for(false, &nv(1), &dir.path().join("missing.json")).is_none());
    }

    #[test]
    fn render_node_vendors_reads_sysfs_layout() {
        use crate::gpu::render_node_vendors;
        let dir = tempfile::tempdir().unwrap();
        for (node, vendor) in [("renderD128", "0x1002\n"), ("renderD129", "0x10de\n")] {
            let dev = dir.path().join(node).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("vendor"), vendor).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("card0").join("device")).unwrap();
        let mut vendors = render_node_vendors(dir.path());
        vendors.sort();
        assert_eq!(vendors, vec!["0x1002".to_owned(), "0x10de".to_owned()]);
        assert!(render_node_vendors(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn kill_rejects_process_outside_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let err = WineRuntime
            .kill(dir.path(), std::process::id())
            .unwrap_err();
        assert!(err.to_string().starts_with("LSW1507"));
    }

    #[test]
    fn diagnostics_on_nonexistent_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-such-prefix");
        let diag = WineRuntime.diagnostics(&missing);
        assert_eq!(diag.id, "wine");
        assert!(!diag.prefix_exists);
        assert!(!diag.prefix_initialized);
    }

    #[test]
    fn diagnostics_on_initialized_looking_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SYSTEM_REG), "WINE REGISTRY Version 2\n").unwrap();
        let diag = WineRuntime.diagnostics(dir.path());
        assert!(diag.prefix_exists);
        assert!(diag.prefix_initialized);
    }

    #[test]
    fn diagnostics_reports_version_and_executable_when_wine_present() {
        if skip_without_wine("diagnostics_reports_version_and_executable_when_wine_present") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let diag = WineRuntime.diagnostics(dir.path());
        assert!(diag.version.is_some_and(|v| !v.is_empty()));
        assert!(diag.executable.is_some_and(|e| e.is_absolute()));
    }

    #[test]
    fn providers_lists_wine_first() {
        let all = providers();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id(), "wine");
    }

    #[test]
    fn gated_prepare_is_idempotent_and_execute_runs_cmd() {
        if skip_without_wine_gate("gated_prepare_is_idempotent_and_execute_runs_cmd") {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("prefix");

        WineRuntime.prepare(&prefix).unwrap();
        WineRuntime.prepare(&prefix).unwrap();

        let diag = WineRuntime.diagnostics(&prefix);
        assert!(diag.prefix_exists);
        assert!(diag.prefix_initialized, "system.reg missing after wineboot");

        let status = WineRuntime
            .execute(&ExecutionRequest {
                program: PathBuf::from("cmd.exe"),
                args: vec!["/c".into(), "exit".into(), "0".into()],
                prefix: prefix.clone(),
                cwd: Some(dir.path().to_path_buf()),
                env: Vec::new(),
                sandbox: None,
                display: DisplayMode::Inherit,
                emulate: None,
            })
            .unwrap();
        assert!(status.success(), "cmd.exe /c exit 0 failed: {status}");
    }
}
