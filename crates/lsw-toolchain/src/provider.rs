use std::fs;

use lsw_config::{ResolvedToolchain, TargetArch};

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

pub(crate) fn run_probe(provider_id: &str, tc: &ResolvedToolchain) -> ProbeReport {
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
