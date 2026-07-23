#[derive(Debug, Clone, Copy)]
pub struct Explanation {
    pub code: &'static str,
    pub summary: &'static str,
    pub hint: &'static str,
}

pub fn explain(code: &str) -> Option<Explanation> {
    let normalized = normalize(code);
    TABLE.iter().find(|e| e.code == normalized).copied()
}

fn normalize(code: &str) -> String {
    let trimmed = code.trim();
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        trimmed.to_uppercase()
    } else {
        format!("LSW{digits}")
    }
}

const TABLE: &[Explanation] = &[
    Explanation {
        code: "LSW1001",
        summary: "a configuration or state file could not be read",
        hint: "check that the path named in the error exists and is readable",
    },
    Explanation {
        code: "LSW1002",
        summary: "a configuration or state file could not be written",
        hint: "check permissions and free space for the path named in the error",
    },
    Explanation {
        code: "LSW1003",
        summary: "a TOML file failed to parse",
        hint: "fix the syntax error at the file and position named in the error",
    },
    Explanation {
        code: "LSW1004",
        summary: "internal serialization of a configuration structure failed",
        hint: "this is an LSW bug; please report it with the full error message",
    },
    Explanation {
        code: "LSW1005",
        summary: "no lsw.toml was found in this directory or any parent",
        hint: "run `lsw init` to scaffold a project, or cd into an existing one",
    },
    Explanation {
        code: "LSW1006",
        summary: "the home directory could not be determined",
        hint: "set $HOME, or run as a user that has a home directory",
    },
    Explanation {
        code: "LSW1007",
        summary: "the environment was created by a newer LSW than this build supports",
        hint: "upgrade LSW, or recreate the environment with `lsw env create --force`",
    },
    Explanation {
        code: "LSW1201",
        summary: "a host path passed to path translation was not absolute",
        hint: "canonicalize the path first, or join it onto the project root",
    },
    Explanation {
        code: "LSW1202",
        summary: "no path mapping covers the given path",
        hint: "keep files under the project root or the environment's drive_c",
    },
    Explanation {
        code: "LSW1203",
        summary: "the string is not a drive-letter Windows path",
        hint: "pass a form like C:\\dir\\file, as produced by `lsw path` or Windows tools",
    },
    Explanation {
        code: "LSW1204",
        summary: "the path contains a non-UTF-8 component",
        hint: "rename the offending file or directory to valid UTF-8",
    },
    Explanation {
        code: "LSW1301",
        summary: "the file could not be read for PE inspection",
        hint: "check that the file exists and is readable",
    },
    Explanation {
        code: "LSW1302",
        summary: "the file has an MZ header but is not a valid PE image",
        hint: "the binary is corrupt or truncated; rebuild or re-download it",
    },
    Explanation {
        code: "LSW1303",
        summary: "the file is not a PE executable",
        hint: "pass a Windows .exe or .dll, such as one produced by `lsw build`",
    },
    Explanation {
        code: "LSW1401",
        summary: "the requested toolchain provider is unavailable on this system",
        hint: "install the provider named in the error, or choose another with --toolchain",
    },
    Explanation {
        code: "LSW1402",
        summary: "the toolchain provider failed its probe compile",
        hint: "read the probe output in the error; the toolchain cannot produce PE binaries",
    },
    Explanation {
        code: "LSW1403",
        summary: "no toolchain provider produced a working Windows PE binary",
        hint: "install llvm-mingw or mingw-w64, then re-run `lsw env create`",
    },
    Explanation {
        code: "LSW1404",
        summary: "an unknown toolchain provider was requested",
        hint: "use a provider named in the error message, or omit --toolchain",
    },
    Explanation {
        code: "LSW1501",
        summary: "the wine runtime was not found on PATH",
        hint: "install wine (e.g. `pacman -S wine` or `apt install wine`)",
    },
    Explanation {
        code: "LSW1502",
        summary: "wine prefix initialization failed",
        hint: "recreate the environment, or run `wineboot -u` with WINEPREFIX set to inspect",
    },
    Explanation {
        code: "LSW1503",
        summary: "the runtime could not spawn a program",
        hint: "check that the binary named in the error exists and is executable",
    },
    Explanation {
        code: "LSW1504",
        summary: "runtime execution failed",
        hint: "read the detail in the error; adjust WINEDEBUG for more diagnostics",
    },
    Explanation {
        code: "LSW1505",
        summary: "a strict sandbox was requested but bubblewrap is not installed",
        hint: "install bubblewrap, or drop --sandbox",
    },
    Explanation {
        code: "LSW1506",
        summary: "a virtual display was requested but xvfb-run is not installed",
        hint: "install xvfb, or run with a real $DISPLAY",
    },
    Explanation {
        code: "LSW1507",
        summary: "the process is not running in this environment",
        hint: "list processes with `lsw ps` to get a valid pid",
    },
    Explanation {
        code: "LSW2001",
        summary: "no active environment is selected for this project",
        hint: "run `lsw use <name>` (or `lsw env create <name>` first)",
    },
    Explanation {
        code: "LSW2002",
        summary: "the named environment does not exist",
        hint: "create it with `lsw env create <name>`, or list with `lsw env list`",
    },
    Explanation {
        code: "LSW2003",
        summary: "an environment with that name already exists",
        hint: "remove it with `lsw env remove <name>`, or choose another name",
    },
    Explanation {
        code: "LSW2004",
        summary: "the target is not something LSW can execute",
        hint: "pass a PE/ELF/script, or force a domain with --host or --windows",
    },
    Explanation {
        code: "LSW2005",
        summary: "the build command failed",
        hint: "re-run `lsw build --verbose` and read the compiler output above",
    },
    Explanation {
        code: "LSW2006",
        summary: "lsw.lock does not match the active environment",
        hint: "refresh the pins with `lsw build --update-lock`, or `lsw env restore`",
    },
    Explanation {
        code: "LSW2007",
        summary: "no build system was detected",
        hint: "add CMakeLists.txt/Cargo.toml/meson.build, or set [build] command in lsw.toml",
    },
    Explanation {
        code: "LSW2008",
        summary: "the target os in lsw.toml is not supported",
        hint: "set os = \"windows\"; LSW only targets Windows",
    },
    Explanation {
        code: "LSW2009",
        summary: "project scaffolding failed",
        hint: "read the detail in the error; check the directory is writable",
    },
    Explanation {
        code: "LSW2010",
        summary: "an io operation failed",
        hint: "check the named path: existence, permissions, free space",
    },
    Explanation {
        code: "LSW2011",
        summary: "a required external tool was not found on PATH",
        hint: "install the tool named in the error message",
    },
    Explanation {
        code: "LSW2012",
        summary: "an invalid environment or project name was given",
        hint: "use a name without slashes, dots-only, or control characters",
    },
    Explanation {
        code: "LSW2013",
        summary: "the build produced a host binary instead of a Windows PE binary",
        hint: "use the generated CMake toolchain, or make [build] honor CC/CXX/CFLAGS/LDFLAGS",
    },
    Explanation {
        code: "LSW2014",
        summary: "no tests were found to run",
        hint: "add add_test(...) to CMakeLists.txt, or set [test] command in lsw.toml",
    },
    Explanation {
        code: "LSW2015",
        summary: "a registry operation failed",
        hint: "check the key path (e.g. HKCU\\Software\\Example\\App) and the output above",
    },
    Explanation {
        code: "LSW2016",
        summary: "the process does not belong to this environment or already exited",
        hint: "list this environment's processes with `lsw ps`",
    },
    Explanation {
        code: "LSW2017",
        summary: "the build directory was configured without the Windows test emulator",
        hint: "remove the build/ directory and re-run `lsw test`",
    },
    Explanation {
        code: "LSW2018",
        summary: "two build artifacts share the same file name",
        hint: "rename a target, or package a single configuration",
    },
    Explanation {
        code: "LSW2019",
        summary: "an SDK with that name is already imported",
        hint: "re-import with `lsw sdk import <name> --force`, or `lsw sdk remove <name>`",
    },
    Explanation {
        code: "LSW2020",
        summary: "the named SDK is not imported",
        hint: "list imported SDKs with `lsw sdk list`",
    },
    Explanation {
        code: "LSW2021",
        summary: "an unsupported verification transport was configured",
        hint: "set transport = \"ssh\" in [verify]; only ssh is implemented",
    },
    Explanation {
        code: "LSW2022",
        summary: "a provider plugin violated the plugin protocol",
        hint: "update or fix the plugin named in the error",
    },
    Explanation {
        code: "LSW2023",
        summary: "the optional lsw daemon is not running",
        hint: "start it with `lswd`; most commands work without the daemon",
    },
    Explanation {
        code: "LSW2024",
        summary: "an unsafe path or name was passed to native verification",
        hint: "use a drive-letter path with segments of [A-Za-z0-9._+-] only",
    },
    Explanation {
        code: "LSW2025",
        summary: "Rust has no GNU-ABI Windows target for this arch",
        hint: "use x86_64, x86, or aarch64 for Rust projects",
    },
    Explanation {
        code: "LSW2026",
        summary: "a Windows service operation failed",
        hint: "read the operation detail in the error message",
    },
    Explanation {
        code: "LSW2027",
        summary: "the compatibility database could not be read or queried",
        hint: "read the detail in the error message",
    },
    Explanation {
        code: "LSW2028",
        summary: "the debug adapter hit a protocol error",
        hint: "restart the debug session; read the detail in the error message",
    },
    Explanation {
        code: "LSW2029",
        summary: "MSIX signing failed",
        hint: "check the signing certificate and key; read the detail in the error",
    },
    Explanation {
        code: "LSW2030",
        summary: "an invalid [sandbox] network value was set",
        hint: "use network = \"host\", \"isolated\", or \"none\"",
    },
    Explanation {
        code: "LSW2031",
        summary: "the crash dump could not be parsed",
        hint: "pass a Windows minidump (.dmp), such as one written by the runtime",
    },
    Explanation {
        code: "LSW2032",
        summary: "the native import probe failed on the remote host",
        hint: "check ssh connectivity to the host named in the error",
    },
    Explanation {
        code: "LSW2033",
        summary: "the package was not found in the mingw-w64 package set",
        hint: "use the upstream library name (e.g. zlib, sqlite3, libpng)",
    },
    Explanation {
        code: "LSW2034",
        summary: "a download failed",
        hint: "check network access and the URL in the error, then retry",
    },
    Explanation {
        code: "LSW2035",
        summary: "a downloaded file failed checksum verification",
        hint: "retry the download; if it persists the mirror is serving a bad file",
    },
    Explanation {
        code: "LSW2036",
        summary: "an archive could not be unpacked",
        hint: "retry the download; the archive may be corrupt",
    },
    Explanation {
        code: "LSW2037",
        summary: "no mingw-w64 package repository exists for this arch",
        hint: "use an arch with a mingw-w64 repo, or vendor the dependency manually",
    },
    Explanation {
        code: "LSW2038",
        summary: "cross-architecture execution needs a Wine build for the target arch",
        hint: "set the env var named in the error to a matching wine (under qemu)",
    },
    Explanation {
        code: "LSW2039",
        summary: "case = \"strict\" found case-insensitive file name collisions",
        hint: "rename the colliding files, or set [filesystem] case = \"native\"",
    },
    Explanation {
        code: "LSW2040",
        summary: "the MSI failed install/uninstall verification in a scratch environment",
        hint: "inspect the msiexec output in the error; rerun `lsw package --target msi --verify`",
    },
    Explanation {
        code: "LSW2041",
        summary: "C# NativeAOT cross-compilation prerequisites are missing",
        hint: "needs x86_64 target, clang, lld-link, and a mingw-w64 sysroot; see the error detail",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn explain_normalizes_and_looks_up() {
        assert_eq!(explain("LSW2004").unwrap().code, "LSW2004");
        assert_eq!(explain("2004").unwrap().code, "LSW2004");
        assert_eq!(explain("lsw2004").unwrap().code, "LSW2004");
        assert!(explain("LSW9999").is_none());
    }

    #[test]
    fn table_is_sorted_and_unique() {
        let codes: Vec<&str> = TABLE.iter().map(|e| e.code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted);
    }

    fn collect_codes(dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_codes(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                let bytes = text.as_bytes();
                for (i, _) in text.match_indices("LSW") {
                    let digits = &bytes[i + 3..];
                    if digits.len() >= 4 && digits[..4].iter().all(u8::is_ascii_digit) {
                        out.insert(text[i..i + 7].to_owned());
                    }
                }
            }
        }
    }

    #[test]
    fn every_emitted_code_has_an_explanation() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut codes = BTreeSet::new();
        for entry in std::fs::read_dir(crates).unwrap().flatten() {
            let src = entry.path().join("src");
            if src.is_dir() {
                collect_codes(&src, &mut codes);
            }
        }
        codes.remove("LSW0000");
        codes.remove("LSW9999");
        assert!(codes.len() >= TABLE.len());
        for code in codes {
            assert!(explain(&code).is_some(), "missing explanation for {code}");
        }
    }
}
