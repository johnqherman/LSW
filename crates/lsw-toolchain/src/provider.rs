use std::fs;
use std::io::Read;

use lsw_config::{ResolvedToolchain, TargetArch};

fn read_magic(path: &std::path::Path) -> std::io::Result<[u8; 2]> {
    let mut buf = [0u8; 2];
    let mut file = fs::File::open(path)?;
    let mut read = 0;
    while read < 2 {
        let n = file.read(&mut buf[read..])?;
        if n == 0 {
            break;
        }
        read += n;
    }
    Ok(buf)
}

use crate::error::{ProbeReport, ToolchainError};
use crate::gnu::{LlvmMingw, MingwGcc};
use crate::util::run_tool;

pub trait ToolchainProvider {
    fn id(&self) -> &'static str;

    fn resolve(&self, arch: TargetArch) -> Result<ResolvedToolchain, ToolchainError>;

    fn probe(&self, arch: TargetArch) -> Result<ProbeReport, ToolchainError> {
        let tc = self.resolve(arch)?;
        Ok(run_probe(self.id(), &tc))
    }
}

pub(crate) fn unavailable(id: &str, detail: &str) -> ToolchainError {
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

pub(crate) fn probe_fixture(
    provider: &str,
) -> std::result::Result<(tempfile::TempDir, std::path::PathBuf), ProbeReport> {
    let dir = tempfile::tempdir().map_err(|e| {
        ProbeReport::failure(
            provider,
            format!("cannot create probe temp directory: {e}"),
            false,
        )
    })?;
    let src = dir.path().join("probe.c");
    fs::write(&src, "int main(void) { return 0; }\n").map_err(|e| {
        ProbeReport::failure(
            provider,
            format!("cannot write probe source {}: {e}", src.display()),
            false,
        )
    })?;
    Ok((dir, src))
}

pub(crate) fn run_probe(provider_id: &str, tc: &ResolvedToolchain) -> ProbeReport {
    let mut report = ProbeReport::failure(provider_id, String::new(), false);

    let (dir, src) = match probe_fixture(provider_id) {
        Ok(v) => v,
        Err(report) => return report,
    };
    let obj = dir.path().join("probe.o");
    let exe = dir.path().join("out.exe");

    match run_tool(&tc.cc, |cmd| {
        cmd.args(&tc.c_flags)
            .arg("-c")
            .arg(&src)
            .arg("-o")
            .arg(&obj);
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

    match read_magic(&exe) {
        Ok(magic) if magic.starts_with(b"MZ") => {
            report.produced_pe = true;
            report.detail = format!("produced PE binary via {}", tc.cc.display());
        }
        Ok(_) => report.detail = "output exists but does not start with the 'MZ' PE magic".into(),
        Err(e) => report.detail = format!("cannot read probe output {}: {e}", exe.display()),
    }
    report
}
