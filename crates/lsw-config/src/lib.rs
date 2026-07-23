pub const PROJECT_MANIFEST: &str = "lsw.toml";
pub const PROJECT_LOCKFILE: &str = "lsw.lock";
pub const ENVIRONMENT_MANIFEST: &str = "env.toml";

pub const ENVIRONMENT_FORMAT_VERSION: u32 = 1;
pub const LOCKFILE_VERSION: u32 = 1;

mod dirs;
mod error;
mod lockfile;
mod manifest;
mod types;

pub use dirs::*;
pub use error::*;
pub use lockfile::*;
pub use manifest::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use crate::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn manifest_roundtrip_preserves_all_fields() {
        let mut m = ProjectManifest::new("hello-win32");
        m.target.api = Some("win10".into());
        m.toolchain.provider = Some("llvm-mingw".into());
        m.toolchain.link = LinkMode::Dynamic;
        m.environment.name = Some("win11-x64".into());
        m.build = Some(CommandSection {
            command: vec!["cmake".into(), "--build".into(), "build".into()],
        });

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_MANIFEST);
        m.save(&path).unwrap();
        let loaded = ProjectManifest::load(&path).unwrap();
        assert_eq!(m, loaded);
        assert_eq!(loaded.toolchain.link, LinkMode::Dynamic);
    }

    #[test]
    fn fresh_manifest_omits_empty_sections() {
        let text = toml::to_string_pretty(&ProjectManifest::new("x")).unwrap();
        for section in ["[verify]", "[env]", "[registry]", "[environment]"] {
            assert!(!text.contains(section), "should not emit empty {section}");
        }
        let back: ProjectManifest = toml::from_str(&text).unwrap();
        assert_eq!(back, ProjectManifest::new("x"));
    }

    #[test]
    fn minimal_manifest_parses_with_defaults() {
        let m: ProjectManifest = toml::from_str("[project]\nname = \"x\"\n").unwrap();
        assert_eq!(m.target.os, "windows");
        assert_eq!(m.target.arch, TargetArch::X86_64);
        assert_eq!(m.runtime.provider, "wine");
        assert_eq!(m.filesystem.project_drive, "C:");
        assert_eq!(m.filesystem.mount_project, "/src");
        assert_eq!(m.toolchain.link, LinkMode::Static);
        assert!(m.build.is_none());
    }

    #[test]
    fn registry_seed_roundtrips_with_type_default() {
        let src = "[project]\nname = \"x\"\n[[registry.seed]]\nkey = \"HKCU\\\\Software\\\\App\"\nname = \"Flag\"\nvalue = \"1\"\ntype = \"dword\"\n[[registry.seed]]\nkey = \"HKCU\\\\Software\\\\App\"\nname = \"Path\"\nvalue = \"C:\\\\x\"\n";
        let m: ProjectManifest = toml::from_str(src).unwrap();
        assert_eq!(m.registry.seed.len(), 2);
        assert_eq!(m.registry.seed[0].kind, "dword");
        assert_eq!(m.registry.seed[1].kind, "string");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_MANIFEST);
        m.save(&path).unwrap();
        assert_eq!(ProjectManifest::load(&path).unwrap(), m);
    }

    #[test]
    fn env_section_roundtrips_vars_and_secrets() {
        let src = "[project]\nname = \"x\"\n[env.vars]\nRUST_LOG = \"debug\"\n[env.secret]\nAPI_TOKEN = \"HOST_API_TOKEN\"\n";
        let m: ProjectManifest = toml::from_str(src).unwrap();
        assert_eq!(m.env.vars.get("RUST_LOG").unwrap(), "debug");
        assert_eq!(m.env.secret.get("API_TOKEN").unwrap(), "HOST_API_TOKEN");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_MANIFEST);
        m.save(&path).unwrap();
        assert_eq!(ProjectManifest::load(&path).unwrap(), m);
    }

    #[test]
    fn link_mode_parses_lowercase() {
        let m: ProjectManifest =
            toml::from_str("[project]\nname = \"x\"\n[toolchain]\nlink = \"dynamic\"\n").unwrap();
        assert_eq!(m.toolchain.link, LinkMode::Dynamic);
    }

    #[test]
    fn toolchain_aot_defaults_false_and_parses() {
        let m: ProjectManifest = toml::from_str("[project]\nname = \"x\"\n").unwrap();
        assert!(!m.toolchain.aot);
        let m: ProjectManifest =
            toml::from_str("[project]\nname = \"x\"\n[toolchain]\naot = true\n").unwrap();
        assert!(m.toolchain.aot);
    }

    #[test]
    fn unknown_manifest_keys_are_rejected() {
        let err = toml::from_str::<ProjectManifest>("[project]\nname = \"x\"\nbogus = 1\n");
        assert!(err.is_err());
    }

    #[test]
    fn discover_walks_upward() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        ProjectManifest::new("demo")
            .save(&root.join(PROJECT_MANIFEST))
            .unwrap();
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let (found_root, m) = ProjectManifest::discover(&nested).unwrap();
        assert_eq!(found_root, root);
        assert_eq!(m.project.name, "demo");
    }

    #[test]
    fn discover_fails_outside_projects() {
        let dir = tempfile::tempdir().unwrap();
        let err = ProjectManifest::discover(dir.path()).unwrap_err();
        assert!(err.to_string().contains("LSW1005"));
    }

    #[test]
    fn environment_manifest_rejects_newer_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ENVIRONMENT_MANIFEST);
        let toml = format!(
            "name = \"e\"\nformat = {}\ntarget_arch = \"x86_64\"\n\
             [toolchain]\nprovider = \"llvm-mingw\"\nversion = \"1\"\ncc = \"/cc\"\ncxx = \"/cxx\"\nsysroot = \"/s\"\n\
             [runtime]\nprovider = \"wine\"\nversion = \"9\"\nexecutable = \"/wine\"\n",
            ENVIRONMENT_FORMAT_VERSION + 1
        );
        fs::write(&path, toml).unwrap();
        let err = EnvironmentManifest::load(&path).unwrap_err();
        assert!(err.to_string().contains("LSW1007"));
    }

    #[test]
    fn lockfile_roundtrip() {
        let lock = Lockfile {
            version: 1,
            environment_format: ENVIRONMENT_FORMAT_VERSION,
            target_arch: TargetArch::X86_64,
            toolchain: LockedComponent {
                provider: "llvm-mingw".into(),
                version: "22.1.6".into(),
                sha256: "ab".repeat(32),
            },
            runtime: LockedComponent {
                provider: "wine".into(),
                version: "11.12".into(),
                sha256: "cd".repeat(32),
            },
            sysroot: LockedComponent {
                provider: "mingw-w64".into(),
                version: "unknown".into(),
                sha256: "ef".repeat(32),
            },
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_LOCKFILE);
        lock.save(&path).unwrap();
        assert_eq!(Lockfile::load(&path).unwrap(), lock);
    }

    #[test]
    fn environment_layout_paths() {
        let l = EnvironmentLayout::new(PathBuf::from("/data/lsw/environments/e1"));
        assert_eq!(
            l.prefix(),
            PathBuf::from("/data/lsw/environments/e1/prefix")
        );
        assert_eq!(
            l.drive_c(),
            PathBuf::from("/data/lsw/environments/e1/prefix/drive_c")
        );
        assert_eq!(l.src().file_name().unwrap(), "src");
        assert_eq!(l.temp().file_name().unwrap(), "Temp");
    }

    #[test]
    fn managed_dirs_cover_the_data_layout() {
        let dirs = Dirs {
            data: PathBuf::from("/data/lsw"),
            config: PathBuf::from("/cfg"),
            cache: PathBuf::from("/cache"),
        };
        assert_eq!(dirs.runtimes(), PathBuf::from("/data/lsw/runtimes"));
        assert_eq!(dirs.toolchains(), PathBuf::from("/data/lsw/toolchains"));
        assert_eq!(dirs.packages(), PathBuf::from("/data/lsw/packages"));
        assert!(dirs.managed_dirs().contains(&dirs.sysroots()));
        assert_eq!(dirs.managed_dirs().len(), 5);
    }

    #[test]
    fn arch_triples() {
        assert_eq!(TargetArch::X86_64.mingw_triple(), "x86_64-w64-mingw32");
        assert_eq!(TargetArch::X86.mingw_triple(), "i686-w64-mingw32");
    }
}
