use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::buildops::{self, BuildOptions};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// Outcome.
pub enum Outcome {
    /// Pass.
    Pass,
    /// Fail.
    Fail,
    /// Not Run.
    NotRun,
}

#[derive(Debug, Serialize)]
/// Component.
pub struct Component {
    /// Label.
    pub label: String,
    /// Outcome.
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// Compat Status.
pub enum CompatStatus {
    /// Local Compatibility Verified.
    LocalCompatibilityVerified,
    /// Local Compatibility Failed.
    LocalCompatibilityFailed,
    /// Not Run.
    NotRun,
}

#[derive(Debug, Serialize)]
/// Test Report.
pub struct TestReport {
    /// Build.
    pub build: Component,
    /// Runtime.
    pub runtime: Component,
    /// Native.
    pub native: Component,
    /// Command.
    pub command: String,
    /// Tests passed.
    pub tests_passed: Option<u32>,
    /// Tests failed.
    pub tests_failed: Option<u32>,
    /// Compatibility.
    pub compatibility: CompatStatus,
}

#[derive(Debug, Default)]
/// Test Options.
pub struct TestOptions {
    /// Headless.
    pub headless: bool,
    /// Junit.
    pub junit: Option<std::path::PathBuf>,
    /// Coverage.
    pub coverage: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestKind {
    Explicit,
    Ctest,
    Cargo,
    Meson,
    Dotnet,
}

/// Test.
pub fn test(project: &Project, env: &Environment, opts: &TestOptions) -> Result<TestReport> {
    const MAX_TEST_OUTPUT: u64 = 64 * 1024 * 1024;
    if opts.coverage && env.manifest.toolchain.provider != "llvm-mingw" {
        return Err(Error::ToolMissing {
            tool: "llvm-mingw".into(),
            fix: "--coverage needs the llvm-mingw toolchain (clang instrumentation); install it with lsw toolchain install llvm-mingw and recreate the environment".into(),
        });
    }
    let build_report = buildops::build(
        project,
        env,
        &BuildOptions {
            coverage: opts.coverage,
            ..BuildOptions::default()
        },
    )?;
    let build = Component {
        label: format!("{}-windows", env.manifest.target_arch),
        outcome: Outcome::Pass,
    };
    let cov_dir = project.root.join("build").join("cov");
    if opts.coverage {
        let _ = std::fs::remove_dir_all(&cov_dir);
        std::fs::create_dir_all(&cov_dir).map_err(|e| Error::io(cov_dir.clone(), e))?;
    }

    let (mut argv, extra_env, kind) = test_command(project, env)?;
    if let Some(junit) = &opts.junit {
        match kind {
            TestKind::Ctest => {
                let abs = std::path::absolute(junit).map_err(|e| Error::io(junit.clone(), e))?;
                argv.push("--output-junit".into());
                argv.push(abs.display().to_string());
            }
            TestKind::Meson => {}
            TestKind::Explicit | TestKind::Cargo | TestKind::Dotnet => {
                tracing::warn!(
                    "--junit is supported for ctest and meson test runs only; no report will be written"
                );
            }
        }
    }
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
    for (k, v) in &extra_env {
        command.env(k, v);
    }
    if opts.coverage {
        let mapper = crate::envops::mapper(env, project);
        let win_dir = mapper.to_windows(&cov_dir)?;
        command.env("LLVM_PROFILE_FILE", format!("{win_dir}\\%p.profraw"));
    }
    if opts.headless {
        command.env("LSW_HEADLESS", "1");
    }

    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::ToolMissing {
                tool: spawn.to_owned(),
                fix: format!("install {spawn} or set [test].command in lsw.toml"),
            }
        } else {
            Error::io(project.root.clone(), e)
        }
    })?;
    let out_rx = child
        .stdout
        .take()
        .map(|s| lsw_toolchain::drain_capped(s, MAX_TEST_OUTPUT));
    let err_rx = child
        .stderr
        .take()
        .map(|s| lsw_toolchain::drain_capped(s, MAX_TEST_OUTPUT));
    let status = child
        .wait()
        .map_err(|e| Error::io(project.root.clone(), e))?;
    let out_stdout = out_rx
        .map(lsw_toolchain::Drain::wait_eof)
        .unwrap_or_default();
    let out_stderr = err_rx
        .map(lsw_toolchain::Drain::wait_eof)
        .unwrap_or_default();

    eprint!("{}", String::from_utf8_lossy(&out_stdout));
    eprint!("{}", String::from_utf8_lossy(&out_stderr));

    let stdout_text = String::from_utf8_lossy(&out_stdout);
    let (tests_passed, tests_failed) = match kind {
        TestKind::Cargo => parse_cargo_summary(&stdout_text),
        TestKind::Meson => parse_meson_summary(&stdout_text),
        TestKind::Dotnet => parse_dotnet_summary(&stdout_text),
        TestKind::Ctest | TestKind::Explicit => parse_ctest_summary(&stdout_text),
    };

    if kind == TestKind::Meson
        && let Some(junit) = &opts.junit
    {
        let log = project.root.join("build/meson-logs/testlog.junit.xml");
        if log.is_file() {
            std::fs::copy(&log, junit).map_err(|e| Error::io(junit.clone(), e))?;
        } else {
            tracing::warn!("meson wrote no testlog.junit.xml; --junit report unavailable");
        }
    }

    if opts.coverage {
        report_coverage(project, env, &cov_dir, &build_report.artifacts);
    }

    let passed = status.success() && tests_failed.is_none_or(|f| f == 0);

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

fn coverage_tool(env: &Environment, name: &str) -> Option<std::path::PathBuf> {
    let sibling = env
        .manifest
        .toolchain
        .cc
        .parent()
        .map(|d| d.join(name))
        .filter(|p| p.is_file());
    sibling.or_else(|| crate::buildops::which(name))
}

fn report_coverage(
    project: &Project,
    env: &Environment,
    cov_dir: &Path,
    artifacts: &[std::path::PathBuf],
) {
    let raws: Vec<std::path::PathBuf> = std::fs::read_dir(cov_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "profraw"))
        .collect();
    if raws.is_empty() {
        eprintln!(
            "[coverage] no .profraw files produced; the tests may not have run instrumented code"
        );
        return;
    }
    let (Some(profdata), Some(llvm_cov)) = (
        coverage_tool(env, "llvm-profdata"),
        coverage_tool(env, "llvm-cov"),
    ) else {
        eprintln!("[coverage] llvm-profdata/llvm-cov not found next to the toolchain or on PATH");
        return;
    };
    let merged = cov_dir.join("merged.profdata");
    let merge = Command::new(&profdata)
        .arg("merge")
        .arg("-sparse")
        .args(&raws)
        .arg("-o")
        .arg(&merged)
        .output();
    match merge {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            eprintln!(
                "[coverage] llvm-profdata merge failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            return;
        }
        Err(e) => {
            eprintln!("[coverage] cannot run llvm-profdata: {e}");
            return;
        }
    }
    let mut cov = Command::new(&llvm_cov);
    cov.arg("report")
        .arg(format!("--instr-profile={}", merged.display()));
    let mut first = true;
    for artifact in artifacts {
        let abs = project.root.join(artifact);
        if abs
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            if first {
                cov.arg(&abs);
                first = false;
            } else {
                cov.arg("-object").arg(&abs);
            }
        }
    }
    if first {
        eprintln!("[coverage] no executables to report on");
        return;
    }
    match cov.output() {
        Ok(out) if out.status.success() => {
            println!("\nCoverage:");
            print!("{}", String::from_utf8_lossy(&out.stdout));
            println!("(profile data: {})", merged.display());
        }
        Ok(out) => eprintln!(
            "[coverage] llvm-cov report failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => eprintln!("[coverage] cannot run llvm-cov: {e}"),
    }
}

type TestPlan = (Vec<String>, Vec<(String, String)>, TestKind);

fn test_command(project: &Project, env: &Environment) -> Result<TestPlan> {
    if let Some(spec) = &project.manifest.test
        && !spec.command.is_empty()
    {
        return Ok((spec.command.clone(), Vec::new(), TestKind::Explicit));
    }
    let build_dir = project.root.join("build");
    if has_ctest_config(&build_dir) {
        if !configured_with_emulator(&build_dir) {
            return Err(Error::TestEmulatorMissing);
        }
        return Ok((
            vec![
                "ctest".into(),
                "--test-dir".into(),
                "build".into(),
                "--output-on-failure".into(),
                "--no-tests=error".into(),
            ],
            Vec::new(),
            TestKind::Ctest,
        ));
    }
    if project.root.join("meson.build").is_file() && build_dir.join("meson-info").is_dir() {
        return Ok((
            vec![
                "meson".into(),
                "test".into(),
                "-C".into(),
                "build".into(),
                "--print-errorlogs".into(),
            ],
            Vec::new(),
            TestKind::Meson,
        ));
    }
    if project.root.join("Cargo.toml").is_file() {
        return Ok((
            cargo_test_argv(env)?,
            cargo_test_env(project, env)?,
            TestKind::Cargo,
        ));
    }
    if crate::dotnetops::has_dotnet_project(&project.root) {
        return dotnet_test_plan(project, env);
    }
    Err(Error::NoTests)
}

fn cargo_test_argv(env: &Environment) -> Result<Vec<String>> {
    let triple =
        env.manifest
            .target_arch
            .rust_gnu_triple()
            .ok_or_else(|| Error::RustTargetUnavailable {
                arch: env.manifest.target_arch.to_string(),
            })?;
    Ok(vec![
        "cargo".into(),
        "test".into(),
        "--target".into(),
        triple.to_owned(),
    ])
}

fn cargo_test_env(project: &Project, env: &Environment) -> Result<Vec<(String, String)>> {
    let triple =
        env.manifest
            .target_arch
            .rust_gnu_triple()
            .ok_or_else(|| Error::RustTargetUnavailable {
                arch: env.manifest.target_arch.to_string(),
            })?;
    let triple_env = triple.to_uppercase().replace('-', "_");
    let mut vars = vec![(
        format!("CARGO_TARGET_{triple_env}_RUNNER"),
        env.manifest.runtime.executable.display().to_string(),
    )];
    let default_linker = format!("{}-gcc", env.manifest.target_arch.mingw_triple());
    if crate::buildops::which(&default_linker).is_none() {
        let tc = crate::buildops::effective_toolchain(env, project);
        let link_args: String = tc
            .c_flags
            .iter()
            .chain(&tc.link_flags)
            .map(|f| format!("-Clink-arg={f}"))
            .collect::<Vec<_>>()
            .join(" ");
        vars.push((
            format!("CARGO_TARGET_{triple_env}_LINKER"),
            tc.cc.display().to_string(),
        ));
        vars.push((format!("CARGO_TARGET_{triple_env}_RUSTFLAGS"), link_args));
    }
    Ok(vars)
}

fn parse_cargo_summary(stdout: &str) -> (Option<u32>, Option<u32>) {
    let mut passed: Option<u32> = None;
    let mut failed: Option<u32> = None;
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("test result:") else {
            continue;
        };
        let take = |marker: &str| -> Option<u32> {
            rest.split(';').find_map(|part| {
                let part = part.trim();
                part.strip_suffix(marker)
                    .and_then(|n| n.trim().rsplit(' ').next())
                    .and_then(|n| n.parse().ok())
            })
        };
        if let Some(p) = take(" passed") {
            passed = Some(passed.unwrap_or(0) + p);
        }
        if let Some(f) = take(" failed") {
            failed = Some(failed.unwrap_or(0) + f);
        }
    }
    (passed, failed)
}

fn parse_meson_summary(stdout: &str) -> (Option<u32>, Option<u32>) {
    let mut result = (None, None);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(n) = line.strip_prefix("Ok:") {
            result.0 = n.trim().parse().ok();
        } else if let Some(n) = line.strip_prefix("Fail:") {
            result.1 = n.trim().parse().ok();
        }
    }
    result
}

fn has_ctest_config(build_dir: &Path) -> bool {
    build_dir.join("CTestTestfile.cmake").is_file()
}

fn configured_with_emulator(build_dir: &Path) -> bool {
    use std::io::Read;
    let path = build_dir.join("CMakeCache.txt");
    let Ok(file) = std::fs::File::open(&path) else {
        return false;
    };
    let mut cache = String::new();
    if file
        .take(16 * 1024 * 1024)
        .read_to_string(&mut cache)
        .is_err()
    {
        return false;
    }
    cache.lines().any(|l| {
        l.starts_with("CMAKE_CROSSCOMPILING_EMULATOR")
            && l.split('=').nth(1).is_some_and(|v| !v.trim().is_empty())
    })
}

fn parse_ctest_summary(stdout: &str) -> (Option<u32>, Option<u32>) {
    let mut result = (None, None);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.split("tests passed, ").nth(1) {
            let failed: Option<u32> = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            let total: Option<u32> = rest.rsplit(' ').next().and_then(|n| n.parse().ok());
            let passed = match (total, failed) {
                (Some(t), Some(f)) => Some(t.saturating_sub(f)),
                _ => None,
            };
            result = (passed, failed);
        } else if line.starts_with("100%") && line.contains("tests passed out of ") {
            let total: Option<u32> = line.rsplit(' ').next().and_then(|n| n.parse().ok());
            if let Some(t) = total {
                result = (Some(t), Some(0));
            }
        }
    }
    result
}

fn dotnet_test_plan(project: &Project, env: &Environment) -> Result<TestPlan> {
    let publish_dir = project.root.join("bin/lsw-publish");
    let test_exe = find_dotnet_test_exe(&publish_dir, &project.manifest.project.name)?;
    let wine = env.manifest.runtime.executable.display().to_string();
    let exe = test_exe.display().to_string();
    let env_vars: Vec<(String, String)> = Vec::new();
    Ok((vec![wine, exe], env_vars, TestKind::Dotnet))
}

fn find_dotnet_test_exe(publish_dir: &Path, project_name: &str) -> Result<std::path::PathBuf> {
    if !publish_dir.is_dir() {
        return Err(Error::NoTests);
    }
    let expected = format!("{project_name}.exe");
    let path = publish_dir.join(&expected);
    if path.is_file() {
        return Ok(path);
    }
    let hit = std::fs::read_dir(publish_dir).ok().and_then(|entries| {
        entries
            .flatten()
            .take(10_000)
            .find(|e| e.file_name().to_string_lossy().ends_with(".exe"))
            .map(|e| e.path())
    });
    hit.ok_or(Error::NoTests)
}

fn parse_dotnet_summary(stdout: &str) -> (Option<u32>, Option<u32>) {
    let mut passed: Option<u32> = None;
    let mut failed: Option<u32> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.starts_with("Passed!") || line.starts_with("Failed!") || line.starts_with("Total:")
        {
            for segment in line.split([',', '-']) {
                let part = segment.trim();
                if let Some(n) = part
                    .strip_prefix("Passed:")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    passed = Some(passed.unwrap_or(0) + n);
                } else if let Some(n) = part
                    .strip_prefix("Failed:")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    failed = Some(failed.unwrap_or(0) + n);
                }
            }
        }
    }
    (passed, failed)
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

        let (p, f) = parse_ctest_summary("100% tests passed out of 1");
        assert_eq!((p, f), (Some(1), Some(0)));
    }

    #[test]
    fn cargo_summary_sums_across_crates() {
        let out = "test result: ok. 3 passed; 0 failed; 0 ignored\n\
                   junk\n\
                   test result: FAILED. 2 passed; 1 failed; 0 ignored";
        assert_eq!(parse_cargo_summary(out), (Some(5), Some(1)));
        assert_eq!(parse_cargo_summary("nothing"), (None, None));
    }

    #[test]
    fn meson_summary_parses_ok_and_fail() {
        let out = "Ok:                 4\nExpected Fail:      0\nFail:               1\n";
        assert_eq!(parse_meson_summary(out), (Some(4), Some(1)));
    }

    #[test]
    fn ctest_summary_uses_the_last_matching_line() {
        let stdout = "old: 100% tests passed, 0 tests failed out of 9\n\
                      ...\n\
                      50% tests passed, 1 tests failed out of 2";
        assert_eq!(parse_ctest_summary(stdout), (Some(1), Some(1)));
    }

    #[test]
    fn dotnet_summary_parses_passed() {
        let out = "Passed!  - Failed:     0, Passed:     5, Skipped:     0, Total:     5";
        assert_eq!(parse_dotnet_summary(out), (Some(5), Some(0)));
    }

    #[test]
    fn dotnet_summary_parses_failures() {
        let out = "Failed!  - Failed:     2, Passed:     3, Skipped:     1, Total:     6";
        assert_eq!(parse_dotnet_summary(out), (Some(3), Some(2)));
    }

    #[test]
    fn dotnet_summary_no_match() {
        assert_eq!(parse_dotnet_summary("no summary"), (None, None));
    }

    #[test]
    fn find_dotnet_test_exe_missing_dir() {
        let result = find_dotnet_test_exe(std::path::Path::new("/nonexistent"), "app");
        assert!(result.is_err());
    }
}
