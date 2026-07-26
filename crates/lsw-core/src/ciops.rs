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
          sudo apt-get install -y wine64 wine32 mingw-w64 cmake ninja-build xvfb
      - uses: dtolnay/rust-toolchain@stable
      - name: Install lsw
        run: cargo install lsw
      - name: Build and test
        run: |
          lsw env create ci
          lsw use ci
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
          sudo apt-get install -y wine64 mingw-w64 cmake ninja-build
      - uses: dtolnay/rust-toolchain@stable
      - name: Install lsw
        run: cargo install lsw
      - name: Verify reproducibility
        run: |
          lsw env create ci
          lsw use ci
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
          sudo apt-get install -y wine64 mingw-w64 cmake ninja-build osslsigncode
      - uses: dtolnay/rust-toolchain@stable
      - name: Install lsw
        run: cargo install lsw
      - name: Build and sign
        env:
          LSW_PFX_B64: ${{{{ secrets.LSW_PFX_B64 }}}}
          LSW_PFX_PASSWORD: ${{{{ secrets.LSW_PFX_PASSWORD }}}}
        run: |
          lsw env create ci
          lsw use ci
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
