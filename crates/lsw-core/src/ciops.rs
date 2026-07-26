use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub fn github_workflow(project_name: &str) -> String {
    let escaped: String = project_name
        .chars()
        .map(|c| match c {
            '\\' => "\\\\".to_owned(),
            '"' => "\\\"".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32),
            c => c.to_string(),
        })
        .collect();
    let project_name = format!("\"{escaped}\"");
    let lsw_version = env!("CARGO_PKG_VERSION");
    format!(
        r#"name: {project_name}

on:
  push:
  pull_request:
  workflow_dispatch:

jobs:
  build:
    name: build + test (Linux, Wine)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install toolchain and runtime
        run: |
          sudo dpkg --add-architecture i386
          sudo apt-get update
          sudo apt-get install -y wine wine64 wine32 mingw-w64 cmake ninja-build xvfb
      - uses: dtolnay/rust-toolchain@stable
      - name: Cache lsw
        uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/lsw
          key: lsw-cli-{lsw_version}-${{{{ runner.os }}}}
      - name: Install lsw
        run: command -v lsw >/dev/null || cargo install lsw@{lsw_version}
      - name: Cache lsw environments
        uses: actions/cache@v4
        with:
          path: |
            ~/.local/share/lsw
            ~/.cache/lsw
          key: lsw-env-${{{{ runner.os }}}}-${{{{ hashFiles('lsw.lock') }}}}
          restore-keys: lsw-env-${{{{ runner.os }}}}-
      - name: Build and test
        run: |
          lsw setup
          lsw build
          lsw test --headless

  reproducible:
    name: reproducible build
    if: github.event_name == 'workflow_dispatch'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install toolchain and runtime
        run: |
          sudo apt-get update
          sudo apt-get install -y wine wine64 mingw-w64 cmake ninja-build
      - uses: dtolnay/rust-toolchain@stable
      - name: Cache lsw
        uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/lsw
          key: lsw-cli-{lsw_version}-${{{{ runner.os }}}}
      - name: Install lsw
        run: command -v lsw >/dev/null || cargo install lsw@{lsw_version}
      - name: Verify reproducibility
        run: |
          lsw setup
          lsw verify --reproducible

  # Signs release binaries with a real certificate. Opt-in via manual
  # dispatch; needs two repository secrets:
  #   LSW_PFX_B64      base64 of your PKCS#12 certificate
  #   LSW_PFX_PASSWORD its passphrase
  sign:
    name: sign artifacts
    if: github.event_name == 'workflow_dispatch'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install toolchain and runtime
        run: |
          sudo apt-get update
          sudo apt-get install -y wine wine64 mingw-w64 cmake ninja-build osslsigncode
      - uses: dtolnay/rust-toolchain@stable
      - name: Cache lsw
        uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/lsw
          key: lsw-cli-{lsw_version}-${{{{ runner.os }}}}
      - name: Install lsw
        run: command -v lsw >/dev/null || cargo install lsw@{lsw_version}
      - name: Build and sign
        env:
          LSW_PFX_B64: ${{{{ secrets.LSW_PFX_B64 }}}}
          LSW_PFX_PASSWORD: ${{{{ secrets.LSW_PFX_PASSWORD }}}}
        run: |
          lsw setup
          lsw build
          printf '%s' "$LSW_PFX_B64" | base64 -d > signing.pfx
          for exe in build/*.exe; do
            lsw sign "$exe" --pfx signing.pfx --pfx-pass-env LSW_PFX_PASSWORD \
              --timestamp-url http://timestamp.digicert.com
          done
          rm -f signing.pfx

  # Native Windows verification (opt-in): needs a self-hosted or hosted
  # Windows runner reachable over SSH from the Linux job, wired via [verify]
  # in lsw.toml. Uncomment and configure to turn WINDOWS_UNAVAILABLE into
  # WINDOWS_VERIFIED.
  #
  # verify-native:
  #   runs-on: windows-latest
  #   steps:
  #     - uses: actions/checkout@v4
  #     - run: echo "run 'lsw verify --native-windows' against this host"
"#
    )
}

pub fn gitlab_pipeline() -> String {
    let lsw_version = env!("CARGO_PKG_VERSION");
    format!(
        r#"stages:
  - build

build-windows:
  stage: build
  image: ubuntu:24.04
  cache:
    - key: lsw-cli-{lsw_version}
      paths:
        - .cargo-bin/
    - key:
        files:
          - lsw.lock
      paths:
        - .lsw-data/
  variables:
    XDG_DATA_HOME: $CI_PROJECT_DIR/.lsw-data/share
    XDG_CACHE_HOME: $CI_PROJECT_DIR/.lsw-data/cache
  before_script:
    - dpkg --add-architecture i386
    - apt-get update
    - apt-get install -y wine wine64 wine32 mingw-w64 cmake ninja-build xvfb curl build-essential
    - test -x .cargo-bin/lsw || (curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal && . "$HOME/.cargo/env" && cargo install lsw@{lsw_version} && mkdir -p .cargo-bin && cp "$HOME/.cargo/bin/lsw" "$HOME/.cargo/bin/lswd" .cargo-bin/)
    - export PATH="$CI_PROJECT_DIR/.cargo-bin:$PATH"
  script:
    - lsw setup
    - lsw build
    - lsw test --headless
  artifacts:
    paths:
      - build/*.exe
      - build/*.dll
"#
    )
}

pub fn init_gitlab(project_root: &Path) -> Result<PathBuf> {
    let path = project_root.join(".gitlab-ci.yml");
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(Error::InitFailed {
            path,
            detail: "pipeline file already exists".into(),
        });
    }
    std::fs::write(&path, gitlab_pipeline()).map_err(|e| Error::io(path.clone(), e))?;
    Ok(path)
}

pub fn init_github(project_root: &Path) -> Result<PathBuf> {
    let name = project_root.file_name().map_or_else(
        || "lsw-project".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let gh = project_root.join(".github");
    let dir = gh.join("workflows");
    for d in [&gh, &dir] {
        if std::fs::symlink_metadata(d).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::InitFailed {
                path: d.clone(),
                detail: "path is a symlink; refusing to write the workflow through it".into(),
            });
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(dir.clone(), e))?;
    let path = dir.join("lsw.yml");
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(Error::InitFailed {
            path,
            detail: "workflow already exists".into(),
        });
    }
    std::fs::write(&path, github_workflow(&name)).map_err(|e| Error::io(path.clone(), e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_workflow_names_the_project_and_key_steps() {
        let yaml = github_workflow("demo");
        assert!(yaml.starts_with("name: \"demo\""));
        assert!(yaml.contains("lsw build"));
        assert!(yaml.contains("lsw test --headless"));
        assert!(yaml.contains("mingw-w64"));
        assert!(yaml.contains("install -y wine wine64"));
        assert!(yaml.contains("lsw setup"));
        assert!(yaml.contains(&format!("cargo install lsw@{}", env!("CARGO_PKG_VERSION"))));
        assert!(yaml.contains("hashFiles('lsw.lock')"));
        assert!(yaml.contains("lsw verify --reproducible"));
        assert!(yaml.contains("workflow_dispatch"));
        assert!(yaml.contains("--pfx-pass-env LSW_PFX_PASSWORD"));
        assert!(yaml.contains("${{ secrets.LSW_PFX_B64 }}"));
    }

    #[test]
    fn init_github_writes_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = init_github(dir.path()).unwrap();
        assert!(path.ends_with(".github/workflows/lsw.yml"));
        assert!(init_github(dir.path()).is_err());
    }
}
