use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lsw_config::{ResolvedToolchain, TargetArch};
use sha2::{Digest, Sha256};

pub const LLVM_MINGW_ID: &str = "llvm-mingw";
pub const MINGW_GCC_ID: &str = "mingw-gcc";

#[derive(Debug, thiserror::Error)]
pub enum ToolchainError {
    #[error(
        "LSW1401: toolchain provider '{id}' is unavailable: {detail}. \
         Possible fix: install the missing tool with your distribution's package manager"
    )]
    ProviderUnavailable { id: String, detail: String },

    #[error(
        "LSW1402: toolchain provider '{id}' failed its probe (could not produce a \
         working Windows PE binary): {detail}. Possible fix: reinstall the provider's \
         compiler and mingw-w64 sysroot, or pick another provider"
    )]
    ProbeFailed { id: String, detail: String },

    #[error(
        "LSW1403: no toolchain provider produced a working Windows PE binary:\n{}\n\
         Possible fixes: install mingw-w64 toolchain or clang+lld",
        format_attempts(attempts)
    )]
    NoWorkingProvider { attempts: Vec<(String, String)> },

    #[error(
        "LSW1404: unknown toolchain provider '{id}'. \
         Possible fix: use one of 'llvm-mingw' or 'mingw-gcc'"
    )]
    UnknownProvider { id: String },
}

fn format_attempts(attempts: &[(String, String)]) -> String {
    attempts
        .iter()
        .map(|(id, detail)| format!("  - {id}: {detail}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub provider: String,
    pub compiled: bool,
    pub linked: bool,
    pub produced_pe: bool,
    pub detail: String,
}

impl ProbeReport {
    pub fn passed(&self) -> bool {
        self.compiled && self.linked && self.produced_pe
    }
}

pub trait ToolchainProvider {
    fn id(&self) -> &'static str;

    fn resolve(&self, arch: TargetArch) -> Result<ResolvedToolchain, ToolchainError>;

    fn probe(&self, arch: TargetArch) -> Result<ProbeReport, ToolchainError> {
        let tc = self.resolve(arch)?;
        Ok(run_probe(self.id(), &tc))
    }
}

pub struct LlvmMingw;

impl ToolchainProvider for LlvmMingw {
    fn id(&self) -> &'static str {
        LLVM_MINGW_ID
    }

    fn resolve(&self, arch: TargetArch) -> Result<ResolvedToolchain, ToolchainError> {
        let triple = arch.mingw_triple();
        let cc = which("clang").ok_or_else(|| unavailable(self.id(), "'clang' not on PATH"))?;
        let cxx = which("clang++").ok_or_else(|| unavailable(self.id(), "'clang++' not on PATH"))?;
        let sysroot = PathBuf::from(format!("/usr/{triple}"));
        if !sysroot.is_dir() {
            return Err(unavailable(
                self.id(),
                &format!("mingw-w64 sysroot {} does not exist", sysroot.display()),
            ));
        }
        let include = sysroot.join("include");
        if !include.join("windows.h").is_file() && !include.join("Windows.h").is_file() {
            return Err(unavailable(
                self.id(),
                &format!(
                    "{} has no windows.h; install the mingw-w64 headers",
                    include.display()
                ),
            ));
        }
        let mut link_flags = vec!["-fuse-ld=lld".to_owned()];
        if let Some(libgcc) = latest_gcc_lib_dir(triple) {
            link_flags.push(format!("-L{}", libgcc.display()));
        }
        Ok(ResolvedToolchain {
            provider: self.id().to_owned(),
            version: compiler_version(&cc),
            c_flags: vec![
                format!("--target={triple}"),
                format!("--sysroot={}", sysroot.display()),
            ],
            link_flags,
            cc,
            cxx,
            sysroot,
        })
    }
}

fn latest_gcc_lib_dir(triple: &str) -> Option<PathBuf> {
    let base = PathBuf::from("/usr/lib/gcc").join(triple);
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("libgcc.a").is_file())
        .collect();
    versions.sort();
    versions.pop()
}

pub struct MingwGcc;

impl ToolchainProvider for MingwGcc {
    fn id(&self) -> &'static str {
        MINGW_GCC_ID
    }

    fn resolve(&self, arch: TargetArch) -> Result<ResolvedToolchain, ToolchainError> {
        let triple = arch.mingw_triple();
        let gcc = format!("{triple}-gcc");
        let gxx = format!("{triple}-g++");
        let cc =
            which(&gcc).ok_or_else(|| unavailable(self.id(), &format!("'{gcc}' not on PATH")))?;
        let cxx =
            which(&gxx).ok_or_else(|| unavailable(self.id(), &format!("'{gxx}' not on PATH")))?;
        Ok(ResolvedToolchain {
            provider: self.id().to_owned(),
            version: compiler_version(&cc),
            c_flags: Vec::new(),
            link_flags: Vec::new(),
            cc,
            cxx,
            sysroot: PathBuf::from(format!("/usr/{triple}")),
        })
    }
}

fn unavailable(id: &str, detail: &str) -> ToolchainError {
    ToolchainError::ProviderUnavailable {
        id: id.to_owned(),
        detail: detail.to_owned(),
    }
}

pub fn providers() -> Vec<Box<dyn ToolchainProvider>> {
    vec![Box::new(LlvmMingw), Box::new(MingwGcc)]
}

pub fn select(
    preferred: Option<&str>,
    arch: TargetArch,
) -> Result<(ResolvedToolchain, ProbeReport), ToolchainError> {
    if let Some(id) = preferred {
        let provider = providers()
            .into_iter()
            .find(|p| p.id() == id)
            .ok_or_else(|| ToolchainError::UnknownProvider { id: id.to_owned() })?;
        let tc = provider.resolve(arch)?;
        let report = run_probe(provider.id(), &tc);
        if !report.passed() {
            return Err(ToolchainError::ProbeFailed {
                id: id.to_owned(),
                detail: report.detail,
            });
        }
        return Ok((tc, report));
    }

    let mut attempts: Vec<(String, String)> = Vec::new();
    for provider in providers() {
        match provider.resolve(arch) {
            Ok(tc) => {
                let report = run_probe(provider.id(), &tc);
                if report.passed() {
                    tracing::debug!(provider = provider.id(), "toolchain probe passed");
                    return Ok((tc, report));
                }
                attempts.push((provider.id().to_owned(), report.detail));
            }
            Err(e) => attempts.push((provider.id().to_owned(), e.to_string())),
        }
    }
    Err(ToolchainError::NoWorkingProvider { attempts })
}

fn run_probe(provider_id: &str, tc: &ResolvedToolchain) -> ProbeReport {
    let mut report = ProbeReport {
        provider: provider_id.to_owned(),
        compiled: false,
        linked: false,
        produced_pe: false,
        detail: String::new(),
    };

    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            report.detail = format!("cannot create probe temp directory: {e}");
            return report;
        }
    };
    let src = dir.path().join("probe.c");
    let obj = dir.path().join("probe.o");
    let exe = dir.path().join("out.exe");
    if let Err(e) = fs::write(&src, "int main(void) { return 0; }\n") {
        report.detail = format!("cannot write probe source {}: {e}", src.display());
        return report;
    }

    match run_tool(&tc.cc, |cmd| {
        cmd.args(&tc.c_flags).arg("-c").arg(&src).arg("-o").arg(&obj);
    }) {
        Ok(stderr) => {
            report.compiled = true;
            report.detail = stderr;
        }
        Err(detail) => {
            report.detail = format!("compile failed: {detail}");
            return report;
        }
    }

    match run_tool(&tc.cc, |cmd| {
        cmd.args(&tc.c_flags)
            .args(&tc.link_flags)
            .arg(&obj)
            .arg("-o")
            .arg(&exe);
    }) {
        Ok(stderr) => {
            report.linked = true;
            report.detail = stderr;
        }
        Err(detail) => {
            report.detail = format!("link failed: {detail}");
            return report;
        }
    }

    match fs::read(&exe) {
        Ok(bytes) if bytes.starts_with(b"MZ") => {
            report.produced_pe = true;
            report.detail = format!("produced PE binary via {}", tc.cc.display());
        }
        Ok(_) => report.detail = "output exists but does not start with the 'MZ' PE magic".into(),
        Err(e) => report.detail = format!("cannot read probe output {}: {e}", exe.display()),
    }
    report
}

fn run_tool(tool: &Path, configure: impl FnOnce(&mut Command)) -> Result<String, String> {
    let mut cmd = Command::new(tool);
    configure(&mut cmd);
    match cmd.output() {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            if out.status.success() {
                Ok(stderr)
            } else {
                Err(format!(
                    "{} exited with {}: {stderr}",
                    tool.display(),
                    out.status
                ))
            }
        }
        Err(e) => Err(format!("cannot execute {}: {e}", tool.display())),
    }
}

pub fn write_cmake_toolchain_file(
    path: &Path,
    tc: &ResolvedToolchain,
    arch: TargetArch,
) -> std::io::Result<()> {
    let processor = match arch {
        TargetArch::X86_64 => "AMD64",
        TargetArch::X86 => "X86",
        TargetArch::Aarch64 => "ARM64",
    };
    let c_flags = tc.c_flags.join(" ");
    let link_flags = tc.link_flags.join(" ");

    let mut text = String::new();
    text.push_str("# Generated by lsw-toolchain; do not edit.\n");
    text.push_str("set(CMAKE_SYSTEM_NAME Windows)\n");
    text.push_str(&format!("set(CMAKE_SYSTEM_PROCESSOR {processor})\n"));
    text.push_str(&format!("set(CMAKE_C_COMPILER \"{}\")\n", tc.cc.display()));
    text.push_str(&format!(
        "set(CMAKE_CXX_COMPILER \"{}\")\n",
        tc.cxx.display()
    ));
    text.push_str(&format!("set(CMAKE_C_FLAGS_INIT \"{c_flags}\")\n"));
    text.push_str(&format!("set(CMAKE_CXX_FLAGS_INIT \"{c_flags}\")\n"));
    text.push_str(&format!("set(CMAKE_EXE_LINKER_FLAGS_INIT \"{link_flags}\")\n"));
    text.push_str(&format!(
        "set(CMAKE_SHARED_LINKER_FLAGS_INIT \"{link_flags}\")\n"
    ));
    text.push_str(&format!(
        "set(CMAKE_FIND_ROOT_PATH \"{}\")\n",
        tc.sysroot.display()
    ));
    text.push_str("set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)\n");
    text.push_str("set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)\n");
    text.push_str("set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)\n");
    if let Some(windres) = which(&format!("{}-windres", arch.mingw_triple())) {
        text.push_str(&format!(
            "set(CMAKE_RC_COMPILER \"{}\")\n",
            windres.display()
        ));
    }
    text.push_str("set(CMAKE_EXECUTABLE_SUFFIX \".exe\")\n");
    fs::write(path, text)
}

pub fn compiler_version(cc: &Path) -> String {
    let Ok(out) = Command::new(cc).arg("--version").output() else {
        return "unknown".to_owned();
    };
    if !out.status.success() {
        return "unknown".to_owned();
    }
    match String::from_utf8_lossy(&out.stdout).lines().next() {
        Some(line) if !line.trim().is_empty() => line.trim().to_owned(),
        _ => "unknown".to_owned(),
    }
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(to_hex(hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(hasher.finalize())
}

fn to_hex(digest: impl AsRef<[u8]>) -> String {
    let digest = digest.as_ref();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "set(CMAKE_CXX_FLAGS_INIT \"--target=x86_64-w64-mingw32 --sysroot=/usr/x86_64-w64-mingw32\")"
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
        assert_eq!(tc.link_flags.first().map(String::as_str), Some("-fuse-ld=lld"));
        for extra in &tc.link_flags[1..] {
            assert!(extra.starts_with("-L"), "unexpected link flag {extra}");
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
        assert!(tc.link_flags.is_empty());
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
            link_flags: vec![],
        };
        let report = run_probe("broken", &tc);
        assert!(!report.compiled);
        assert!(!report.linked);
        assert!(!report.produced_pe);
        assert!(report.detail.contains("compile failed"));
    }
}
