use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::buildops::{self, BuildOptions};
use crate::configops::{self, Severity};
use crate::depsops::{self, DepKind, DepNode};
use crate::envops;
use crate::error::{Error, Result};
use crate::project::Project;
use crate::runops::{self, Display, Domain, Sandbox};
use crate::testops::{self, Outcome, TestOptions};
use lsw_config::Dirs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
/// Step Status.
pub enum StepStatus {
    /// Pass.
    Pass,
    /// Fail.
    Fail,
    /// Skip.
    Skip,
}

#[derive(Debug, Serialize)]
/// Check Step.
pub struct CheckStep {
    /// Name.
    pub name: &'static str,
    /// Status.
    pub status: StepStatus,
    /// Detail.
    pub detail: String,
}

#[derive(Debug, Serialize)]
/// Check Report.
pub struct CheckReport {
    /// Steps.
    pub steps: Vec<CheckStep>,
    /// Ok.
    pub ok: bool,
}

#[derive(Debug, Default)]
/// Check Options.
pub struct CheckOptions {
    /// Headless.
    pub headless: bool,
}

fn first_line(e: &Error) -> String {
    let s = e.to_string();
    s.lines().next().unwrap_or_default().to_owned()
}

const STEP_NAMES: &[&str] = &[
    "configuration",
    "environment",
    "build",
    "wine execution",
    "dependencies",
    "hardening",
];

struct Recorder<'a> {
    steps: Vec<CheckStep>,
    progress: &'a mut dyn FnMut(&CheckStep),
}

impl Recorder<'_> {
    fn record(&mut self, name: &'static str, status: StepStatus, detail: String) {
        let step = CheckStep {
            name,
            status,
            detail,
        };
        (self.progress)(&step);
        self.steps.push(step);
    }

    fn skip_remaining(&mut self) {
        for name in &STEP_NAMES[self.steps.len()..] {
            self.record(name, StepStatus::Skip, "skipped (earlier failure)".into());
        }
    }

    fn finish(self) -> CheckReport {
        let ok = self.steps.iter().all(|s| s.status != StepStatus::Fail);
        CheckReport {
            steps: self.steps,
            ok,
        }
    }
}

/// Check.
pub fn check(
    dirs: &Dirs,
    start: &Path,
    opts: &CheckOptions,
    progress: &mut dyn FnMut(&CheckStep),
) -> Result<CheckReport> {
    let mut r = Recorder {
        steps: Vec::new(),
        progress,
    };

    let project = match Project::discover(start) {
        Ok(p) => p,
        Err(e) => {
            r.record(
                "configuration",
                StepStatus::Fail,
                format!("{} (run: lsw setup)", first_line(&e)),
            );
            r.skip_remaining();
            return Ok(r.finish());
        }
    };
    let findings = configops::lint(&project.manifest);
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warns = findings.len() - errors;
    if errors > 0 {
        r.record(
            "configuration",
            StepStatus::Fail,
            format!("{errors} invalid setting(s) in lsw.toml (see: lsw config check)"),
        );
        r.skip_remaining();
        return Ok(r.finish());
    }
    let config_detail = if warns > 0 {
        format!("lsw.toml valid, {warns} warning(s) (see: lsw config check)")
    } else {
        "lsw.toml valid".to_owned()
    };
    r.record("configuration", StepStatus::Pass, config_detail);

    let env = match envops::resolve_active(dirs, &project) {
        Ok(env) => env,
        Err(e) => {
            r.record(
                "environment",
                StepStatus::Fail,
                format!("{} (run: lsw setup)", first_line(&e)),
            );
            r.skip_remaining();
            return Ok(r.finish());
        }
    };
    let m = &env.manifest;
    r.record(
        "environment",
        StepStatus::Pass,
        format!(
            "{} ({} {}, {} {})",
            env.name,
            m.toolchain.provider,
            m.toolchain.version,
            m.runtime.provider,
            m.runtime.version
        ),
    );

    let build = match buildops::build(&project, &env, &BuildOptions::default()) {
        Ok(b) => b,
        Err(e) => {
            r.record("build", StepStatus::Fail, first_line(&e));
            r.skip_remaining();
            return Ok(r.finish());
        }
    };
    r.record(
        "build",
        StepStatus::Pass,
        format!(
            "{} artifact(s) via {}",
            build.artifacts.len(),
            build.system.label()
        ),
    );

    let (status, detail) = wine_execution(&project, &env, opts, &build.artifacts);
    r.record("wine execution", status, detail);

    let (status, detail) = dependencies(&project, &env, &build.artifacts);
    r.record("dependencies", status, detail);

    let (status, detail) = hardening(&project, &build.artifacts);
    r.record("hardening", status, detail);

    Ok(r.finish())
}

fn pe_artifacts(artifacts: &[PathBuf]) -> impl Iterator<Item = &PathBuf> {
    artifacts.iter().filter(|a| {
        a.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
    })
}

fn wine_execution(
    project: &Project,
    env: &envops::Environment,
    opts: &CheckOptions,
    artifacts: &[PathBuf],
) -> (StepStatus, String) {
    match testops::test(
        project,
        env,
        &TestOptions {
            headless: opts.headless,
            junit: None,
            coverage: false,
        },
    ) {
        Ok(t) if t.runtime.outcome == Outcome::Pass => {
            let detail = match t.tests_passed {
                Some(n) => format!("{n} test(s) passed under wine"),
                None => "test command succeeded under wine".to_owned(),
            };
            (StepStatus::Pass, detail)
        }
        Ok(t) => (
            StepStatus::Fail,
            format!("test command failed under wine ({})", t.command),
        ),
        Err(Error::NoTests) => smoke_run(project, env, opts, artifacts),
        Err(e) => (StepStatus::Fail, first_line(&e)),
    }
}

fn smoke_run(
    project: &Project,
    env: &envops::Environment,
    opts: &CheckOptions,
    artifacts: &[PathBuf],
) -> (StepStatus, String) {
    let console_exe = artifacts
        .iter()
        .filter(|a| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
        .find(|a| {
            matches!(
                lsw_pe::detect(&project.root.join(a)),
                Ok(lsw_pe::BinaryKind::Pe(info)) if info.subsystem == lsw_pe::Subsystem::Console
            )
        });
    let Some(exe) = console_exe else {
        return (
            StepStatus::Skip,
            "no tests configured and no console .exe to smoke-run".to_owned(),
        );
    };
    let display = if opts.headless {
        Display::Headless
    } else {
        Display::Auto
    };
    match runops::run(
        env,
        Some(project),
        &project.root.join(exe),
        &[],
        Domain::Windows,
        Sandbox::None,
        display,
    ) {
        Ok(report) if report.status.success() => (
            StepStatus::Pass,
            format!(
                "no tests configured; {} ran under wine (exit 0)",
                exe.display()
            ),
        ),
        Ok(report) => (
            StepStatus::Fail,
            format!(
                "smoke run failed: {} exited with {:?}",
                exe.display(),
                report.status.code()
            ),
        ),
        Err(e) => (StepStatus::Fail, first_line(&e)),
    }
}

fn missing_deps(node: &DepNode, out: &mut Vec<String>) {
    if node.kind == DepKind::Missing {
        out.push(node.name.clone());
    }
    for child in &node.children {
        missing_deps(child, out);
    }
}

fn dependencies(
    project: &Project,
    env: &envops::Environment,
    artifacts: &[PathBuf],
) -> (StepStatus, String) {
    let mut missing = Vec::new();
    let mut scanned = 0usize;
    for artifact in pe_artifacts(artifacts) {
        match depsops::tree(Some(env), &project.root.join(artifact)) {
            Ok(root) => {
                scanned += 1;
                missing_deps(&root, &mut missing);
            }
            Err(e) => return (StepStatus::Fail, first_line(&e)),
        }
    }
    if scanned == 0 {
        return (StepStatus::Skip, "no PE artifacts to scan".to_owned());
    }
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        (
            StepStatus::Pass,
            format!("all DLL imports resolved ({scanned} artifact(s))"),
        )
    } else {
        (
            StepStatus::Fail,
            format!("unresolved DLLs: {}", missing.join(", ")),
        )
    }
}

fn hardening(project: &Project, artifacts: &[PathBuf]) -> (StepStatus, String) {
    let mut weak = Vec::new();
    let mut scanned = 0usize;
    for artifact in pe_artifacts(artifacts) {
        match crate::auditops::audit(&project.root.join(artifact)) {
            Ok(report) => {
                scanned += 1;
                if !report.hardened {
                    weak.push(artifact.display().to_string());
                }
            }
            Err(e) => return (StepStatus::Fail, first_line(&e)),
        }
    }
    if scanned == 0 {
        return (StepStatus::Skip, "no PE artifacts to audit".to_owned());
    }
    if weak.is_empty() {
        (
            StepStatus::Pass,
            format!("ASLR + DEP enabled ({scanned} artifact(s))"),
        )
    } else {
        (
            StepStatus::Fail,
            format!("missing ASLR or DEP: {}", weak.join(", ")),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_extracts_first() {
        let e = Error::NoBuildSystem;
        let fl = first_line(&e);
        assert!(fl.starts_with("LSW2007"));
        assert!(!fl.contains('\n'));
    }

    #[test]
    fn first_line_multiline_error() {
        let e = Error::NoActiveEnvironment;
        let full = e.to_string();
        assert!(full.contains('\n'));
        let fl = first_line(&e);
        assert!(!fl.contains('\n'));
        assert!(fl.contains("LSW2001"));
    }

    #[test]
    fn pe_artifacts_filters_extensions() {
        let files = vec![
            PathBuf::from("app.exe"),
            PathBuf::from("lib.dll"),
            PathBuf::from("readme.txt"),
            PathBuf::from("helper.DLL"),
            PathBuf::from("main.EXE"),
            PathBuf::from("data.json"),
        ];
        let pe: Vec<_> = pe_artifacts(&files).collect();
        assert_eq!(pe.len(), 4);
        assert!(pe.contains(&&PathBuf::from("app.exe")));
        assert!(pe.contains(&&PathBuf::from("lib.dll")));
        assert!(pe.contains(&&PathBuf::from("helper.DLL")));
        assert!(pe.contains(&&PathBuf::from("main.EXE")));
    }

    #[test]
    fn pe_artifacts_empty_list() {
        let files: Vec<PathBuf> = vec![];
        assert_eq!(pe_artifacts(&files).count(), 0);
    }

    #[test]
    fn missing_deps_collects_missing() {
        let tree = DepNode {
            name: "root.dll".into(),
            kind: DepKind::System,
            path: None,
            children: vec![
                DepNode {
                    name: "found.dll".into(),
                    kind: DepKind::System,
                    path: None,
                    children: vec![],
                },
                DepNode {
                    name: "gone.dll".into(),
                    kind: DepKind::Missing,
                    path: None,
                    children: vec![],
                },
                DepNode {
                    name: "parent.dll".into(),
                    kind: DepKind::System,
                    path: None,
                    children: vec![DepNode {
                        name: "nested_gone.dll".into(),
                        kind: DepKind::Missing,
                        path: None,
                        children: vec![],
                    }],
                },
            ],
        };
        let mut out = Vec::new();
        missing_deps(&tree, &mut out);
        assert_eq!(out, vec!["gone.dll", "nested_gone.dll"]);
    }

    #[test]
    fn missing_deps_none_missing() {
        let tree = DepNode {
            name: "root.dll".into(),
            kind: DepKind::System,
            path: None,
            children: vec![],
        };
        let mut out = Vec::new();
        missing_deps(&tree, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn recorder_skip_remaining_fills_rest() {
        let mut r = Recorder {
            steps: Vec::new(),
            progress: &mut |_| {},
        };
        r.record("configuration", StepStatus::Fail, "bad".into());
        r.skip_remaining();
        let report = r.finish();
        assert!(!report.ok);
        assert_eq!(report.steps.len(), STEP_NAMES.len());
        assert_eq!(report.steps[0].status, StepStatus::Fail);
        for step in &report.steps[1..] {
            assert_eq!(step.status, StepStatus::Skip);
        }
    }

    #[test]
    fn recorder_all_pass() {
        let mut r = Recorder {
            steps: Vec::new(),
            progress: &mut |_| {},
        };
        for name in STEP_NAMES {
            r.record(name, StepStatus::Pass, "ok".into());
        }
        let report = r.finish();
        assert!(report.ok);
        assert_eq!(report.steps.len(), STEP_NAMES.len());
    }

    #[test]
    fn step_status_serializes() {
        assert_eq!(serde_json::to_string(&StepStatus::Pass).unwrap(), "\"pass\"");
        assert_eq!(serde_json::to_string(&StepStatus::Fail).unwrap(), "\"fail\"");
        assert_eq!(serde_json::to_string(&StepStatus::Skip).unwrap(), "\"skip\"");
    }
}
