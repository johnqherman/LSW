mod details;
mod detect;
mod error;
mod hardening;
mod imports;
mod resources;
mod timestamp;
mod types;

pub use details::*;
pub use detect::*;
pub use error::*;
pub use hardening::*;
pub use imports::*;
pub use resources::*;
pub use timestamp::*;
pub use types::*;

pub(crate) const MZ_MAGIC: &[u8; 2] = b"MZ";

#[cfg(test)]
use resources::{parse_manifest, parse_version};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use crate::*;
    use std::io::Write;
    use std::process::Command;

    const MINGW_GCC: &str = "x86_64-w64-mingw32-gcc";

    fn write_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    fn build_fixture_exe(dir: &tempfile::TempDir) -> Option<PathBuf> {
        if Command::new(MINGW_GCC).arg("--version").output().is_err() {
            eprintln!("skipping: {MINGW_GCC} not found on PATH");
            return None;
        }
        let src = write_file(
            dir,
            "tick.c",
            b"#include <windows.h>\nint main(void) { return (int)(GetTickCount() & 1); }\n",
        );
        let exe = dir.path().join("tick.exe");
        let out = Command::new(MINGW_GCC)
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("failed to spawn mingw gcc");
        assert!(
            out.status.success(),
            "{MINGW_GCC} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(exe)
    }

    #[test]
    fn detect_real_pe_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        match detect(&exe).unwrap() {
            BinaryKind::Pe(info) => {
                assert_eq!(info.format, PeFormat::Pe32Plus);
                assert_eq!(info.machine, Machine::X86_64);
                assert_eq!(info.subsystem, Subsystem::Console);
            }
            other => panic!("expected PE, got {other:?}"),
        }
    }

    #[test]
    fn imports_real_pe_fixture_lists_kernel32() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let dlls = imports(&exe).unwrap();
        assert!(
            dlls.iter().any(|d| d.eq_ignore_ascii_case("kernel32.dll")),
            "kernel32.dll not found in {dlls:?}"
        );
        for (i, a) in dlls.iter().enumerate() {
            for b in &dlls[i + 1..] {
                assert!(!a.eq_ignore_ascii_case(b), "duplicate DLL entry {a}");
            }
        }
    }

    #[test]
    fn detect_elf_via_current_exe() {
        let me = std::env::current_exe().unwrap();
        assert_eq!(detect(&me).unwrap(), BinaryKind::Elf);
    }

    #[test]
    fn detect_script_via_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "run.sh", b"#!/bin/sh\necho hi\n");
        assert_eq!(detect(&path).unwrap(), BinaryKind::Script);
    }

    #[test]
    fn detect_unknown_for_random_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "noise.bin", &[0x00, 0xde, 0xad, 0xbe, 0xef, 0x42]);
        assert_eq!(detect(&path).unwrap(), BinaryKind::Unknown);
    }

    #[test]
    fn detect_empty_file_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "empty", b"");
        assert_eq!(detect(&path).unwrap(), BinaryKind::Unknown);
    }

    #[test]
    fn detect_truncated_mz_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "trunc.exe", b"MZ");
        let err = detect(&path).unwrap_err();
        assert!(matches!(err, PeError::MalformedPe { .. }), "got {err:?}");
        assert!(err.to_string().starts_with("LSW1302"));
    }

    #[test]
    fn detect_mz_with_garbage_headers_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0u8; 128];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        let path = write_file(&dir, "garbage.exe", &bytes);
        assert!(matches!(
            detect(&path).unwrap_err(),
            PeError::MalformedPe { .. }
        ));
    }

    #[test]
    fn detect_missing_file_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = detect(&dir.path().join("absent.exe")).unwrap_err();
        assert!(matches!(err, PeError::Io { .. }), "got {err:?}");
        assert!(err.to_string().starts_with("LSW1301"));
    }

    #[test]
    fn coff_timestamp_can_be_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        set_coff_timestamp(&exe, 0).unwrap();
        assert_eq!(coff_timestamp(&exe).unwrap(), 0);
        set_coff_timestamp(&exe, 0).unwrap();
        assert_eq!(coff_timestamp(&exe).unwrap(), 0);
    }

    #[test]
    fn hardening_reads_dll_characteristics_of_a_real_pe() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let h = hardening(&exe).unwrap();
        assert!(h.aslr, "mingw enables DYNAMICBASE by default");
        assert!(h.dep, "mingw enables NXCOMPAT by default");
        assert!(!h.signed, "a freshly built exe is unsigned");
    }

    #[test]
    fn hardening_rejects_non_pe() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::env::current_exe().unwrap_or_else(|_| dir.path().join("x"));
        let _ = hardening(&me);
        let txt = write_file(&dir, "n.txt", b"not a pe");
        assert!(matches!(
            hardening(&txt).unwrap_err(),
            PeError::NotPe { .. }
        ));
    }

    #[test]
    fn parse_manifest_extracts_execution_level_and_dpi() {
        let xml = br#"<assembly><trustInfo><security><requestedPrivileges>
            <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
            </requestedPrivileges></security></trustInfo>
            <asmv3:windowsSettings><dpiAware>true</dpiAware></asmv3:windowsSettings></assembly>"#;
        let mut r = Resources::default();
        parse_manifest(xml, &mut r);
        assert_eq!(r.execution_level.as_deref(), Some("requireAdministrator"));
        assert_eq!(r.dpi_aware.as_deref(), Some("true"));
        assert!(r.manifest.is_some());
    }

    #[test]
    fn parse_version_pairs_known_keys() {
        let mut wide: Vec<u16> = Vec::new();
        for s in ["FileVersion", "1.2.3.4", "ProductName", "Demo", "junk"] {
            wide.extend(s.encode_utf16());
            wide.push(0);
        }
        let bytes: Vec<u8> = wide.iter().flat_map(|u| u.to_le_bytes()).collect();
        let mut map = std::collections::BTreeMap::new();
        parse_version(&bytes, &mut map);
        assert_eq!(map.get("FileVersion").unwrap(), "1.2.3.4");
        assert_eq!(map.get("ProductName").unwrap(), "Demo");
    }

    #[test]
    fn resources_on_resourceless_pe_is_empty_ok() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let r = resources(&exe).unwrap();
        assert!(r.manifest.is_none());
        assert!(!r.has_icon);
    }

    #[test]
    fn parser_never_panics_on_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let mut cases: Vec<Vec<u8>> = vec![
            vec![],
            b"MZ".to_vec(),
            b"MZ\x00\x00".to_vec(),
            {
                let mut v = vec![0u8; 0x40];
                v[0] = b'M';
                v[1] = b'Z';
                v[0x3C] = 0xFF;
                v[0x3D] = 0xFF;
                v
            },
            {
                let mut v = vec![0u8; 0x80];
                v[0] = b'M';
                v[1] = b'Z';
                v[0x3C] = 0x40;
                v[0x40] = b'P';
                v[0x41] = b'E';
                v
            },
        ];
        for i in 0..64u8 {
            cases.push(vec![i; (i as usize) * 3 + 1]);
        }
        for (n, bytes) in cases.iter().enumerate() {
            let path = write_file(&dir, &format!("case{n}.bin"), bytes);
            let _ = detect(&path);
            let _ = imports(&path);
            let _ = hardening(&path);
            let _ = exports(&path);
            let _ = details(&path);
            let _ = imported_symbols(&path);
            let _ = coff_timestamp(&path);
            let _ = resources(&path);
        }
    }

    #[test]
    fn imported_symbols_lists_named_functions() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let symbols = imported_symbols(&exe).unwrap();
        assert!(!symbols.is_empty(), "expected named imports");
        assert!(
            symbols
                .iter()
                .any(|(dll, func)| dll.eq_ignore_ascii_case("KERNEL32.dll")
                    && func == "GetTickCount"),
            "GetTickCount import not found in {symbols:?}"
        );
    }

    #[test]
    fn imports_rejects_non_pe() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "script.sh", b"#!/bin/sh\n");
        let err = imports(&path).unwrap_err();
        assert!(matches!(err, PeError::NotPe { .. }), "got {err:?}");
        assert!(err.to_string().starts_with("LSW1303"));
    }

    #[test]
    fn imports_rejects_elf() {
        let me = std::env::current_exe().unwrap();
        assert!(matches!(imports(&me).unwrap_err(), PeError::NotPe { .. }));
    }

    #[test]
    fn imports_truncated_mz_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "trunc.exe", b"MZ");
        assert!(matches!(
            imports(&path).unwrap_err(),
            PeError::MalformedPe { .. }
        ));
    }

    #[test]
    fn imports_missing_file_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            imports(&dir.path().join("absent.exe")).unwrap_err(),
            PeError::Io { .. }
        ));
    }

    #[test]
    fn machine_and_subsystem_mappings() {
        assert_eq!(Machine::from_coff(0x014c), Machine::X86);
        assert_eq!(Machine::from_coff(0x8664), Machine::X86_64);
        assert_eq!(Machine::from_coff(0xaa64), Machine::Aarch64);
        assert_eq!(Machine::from_coff(0x01c4), Machine::Other(0x01c4));
        assert_eq!(Subsystem::from_pe(2), Subsystem::Gui);
        assert_eq!(Subsystem::from_pe(3), Subsystem::Console);
        assert_eq!(Subsystem::from_pe(1), Subsystem::Other(1));
    }

    #[test]
    fn error_messages_carry_stable_ids_and_paths() {
        let io = PeError::io(
            Path::new("/x/y.exe"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        assert!(io.to_string().contains("LSW1301"));
        assert!(io.to_string().contains("/x/y.exe"));

        let mal = PeError::malformed(Path::new("/x/y.exe"), "bad header");
        assert!(mal.to_string().contains("LSW1302"));
        assert!(mal.to_string().contains("bad header"));

        let not_pe = PeError::NotPe {
            path: PathBuf::from("/x/y.sh"),
        };
        assert!(not_pe.to_string().contains("LSW1303"));
        assert!(not_pe.to_string().contains("/x/y.sh"));
    }
}
