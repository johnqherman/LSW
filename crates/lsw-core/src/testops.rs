use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::buildops::{self, BuildOptions};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Outcome {
    Pass,
    Fail,
    NotRun,
}

#[derive(Debug, Serialize)]
pub struct Component {
    pub label: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompatStatus {
    LocalCompatibilityVerified,
    LocalCompatibilityFailed,
    NotRun,
}

#[derive(Debug, Serialize)]
pub struct TestReport {
    pub build: Component,
    pub runtime: Component,
    pub native: Component,
    pub command: String,
    pub tests_passed: Option<u32>,
    pub tests_failed: Option<u32>,
    pub compatibility: CompatStatus,
}

#[derive(Debug, Default)]
pub struct TestOptions {
    pub headless: bool,
}

pub fn test(project: &Project, env: &Environment, opts: &TestOptions) -> Result<TestReport> {
    let build_report = buildops::build(
        project,
        env,
        &BuildOptions {
            system: None,
            update_lock: false,
            reproducible: false,
        },
    )?;
    let build = Component {
        label: format!("{}-windows", env.manifest.target_arch),
        outcome: Outcome::Pass,
    };

    let argv = test_command(project)?;
    let rendered = argv.join(" ");
    let (program, args) = argv.split_first().expect("test_command never empty");

    let no_display = std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty());
    let use_xvfb = opts.headless && no_display && lsw_runtime::find_xvfb_run().is_some();

    let (spawn, spawn_args): (&str, Vec<String>) = if use_xvfb {
        let mut v = vec!["-a".to_owned(), "--".to_owned(), program.clone()];
        v.extend(args.iter().cloned());
        ("xvfb-run", v)
    } else {
        (program.as_str(), args.to_vec())
    };

    let mut command = Command::new(spawn);
    command.args(&spawn_args).current_dir(&project.root);
    lsw_runtime::scrub_wine_env(&mut command);
    for (k, v) in lsw_runtime::base_env(&env.layout.prefix()) {
        command.env(k, v);
    }
    if opts.headless {
        command.env("LSW_HEADLESS", "1");
    }

    let output = command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing {
                tool: spawn.to_owned(),
                fix: format!("install {spawn} or set [test].command in lsw.toml"),
            }
        } else {
            Error::io(project.root.clone(), e)
        }
    })?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    let (tests_passed, tests_failed) =
        parse_ctest_summary(&String::from_utf8_lossy(&output.stdout));

    let passed = output.status.success() && tests_failed.is_none_or(|f| f == 0);

    let _ = build_report;
    Ok(TestReport {
        build,
        runtime: Component {
            label: format!(
                "{}-{}",
                env.manifest.runtime.provider, env.manifest.runtime.version
            ),
            outcome: if passed { Outcome::Pass } else { Outcome::Fail },
        },
        native: Component {
            label: "not configured".into(),
            outcome: Outcome::NotRun,
        },
        command: rendered,
        tests_passed,
        tests_failed,
        compatibility: if passed {
            CompatStatus::LocalCompatibilityVerified
        } else {
            CompatStatus::LocalCompatibilityFailed
        },
    })
}

fn test_command(project: &Project) -> Result<Vec<String>> {
    if let Some(spec) = &project.manifest.test
        && !spec.command.is_empty()
    {
        return Ok(spec.command.clone());
    }
    let build_dir = project.root.join("build");
    if has_ctest_config(&build_dir) {
        if !configured_with_emulator(&build_dir) {
            return Err(Error::TestEmulatorMissing);
        }
        return Ok(vec![
            "ctest".into(),
            "--test-dir".into(),
            "build".into(),
            "--output-on-failure".into(),
            "--no-tests=error".into(),
        ]);
    }
    Err(Error::NoTests)
}

fn has_ctest_config(build_dir: &Path) -> bool {
    build_dir.join("CTestTestfile.cmake").is_file()
}

fn configured_with_emulator(build_dir: &Path) -> bool {
    std::fs::read_to_string(build_dir.join("CMakeCache.txt"))
        .map(|cache| {
            cache.lines().any(|l| {
                l.starts_with("CMAKE_CROSSCOMPILING_EMULATOR")
                    && l.split('=').nth(1).is_some_and(|v| !v.trim().is_empty())
            })
        })
        .unwrap_or(false)
}

fn parse_ctest_summary(stdout: &str) -> (Option<u32>, Option<u32>) {
    let mut result = (None, None);
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line.split("tests passed, ").nth(1) else {
            continue;
        };
        let failed: Option<u32> = rest.split_whitespace().next().and_then(|n| n.parse().ok());
        let total: Option<u32> = rest.rsplit(' ').next().and_then(|n| n.parse().ok());
        let passed = match (total, failed) {
            (Some(t), Some(f)) => Some(t.saturating_sub(f)),
            _ => None,
        };
        result = (passed, failed);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctest_summary_parses() {
        let (p, f) = parse_ctest_summary("junk\n100% tests passed, 0 tests failed out of 3\nmore");
        assert_eq!((p, f), (Some(3), Some(0)));

        let (p, f) = parse_ctest_summary("50% tests passed, 2 tests failed out of 4");
        assert_eq!((p, f), (Some(2), Some(2)));

        assert_eq!(parse_ctest_summary("no summary here"), (None, None));
    }

    #[test]
    fn ctest_summary_uses_the_last_matching_line() {
        let stdout = "old: 100% tests passed, 0 tests failed out of 9\n\
                      ...\n\
                      50% tests passed, 1 tests failed out of 2";
        assert_eq!(parse_ctest_summary(stdout), (Some(1), Some(1)));
    }
}
