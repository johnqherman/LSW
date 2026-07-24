use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

const MAX_TOOL_OUTPUT: u64 = 16 * 1024 * 1024;

pub(crate) fn capped_output(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let so = child.stdout.take().expect("piped stdout");
    let mut se = child.stderr.take().expect("piped stderr");
    let drain = move |mut p: std::process::ChildStdout| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = p.by_ref().take(MAX_TOOL_OUTPUT).read_to_end(&mut b);
            let _ = std::io::copy(&mut p, &mut std::io::sink());
            b
        })
    };
    let h_out = drain(so);
    let h_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.by_ref().take(MAX_TOOL_OUTPUT).read_to_end(&mut b);
        let _ = std::io::copy(&mut se, &mut std::io::sink());
        b
    });
    let status = child.wait()?;
    let stdout = h_out.join().unwrap_or_default();
    let stderr = h_err.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

pub fn compiler_version(cc: &Path) -> String {
    let Ok(out) = capped_output(Command::new(cc).arg("--version")) else {
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

pub(crate) fn which(name: &str) -> Option<PathBuf> {
    for dir in extra_toolchain_dirs() {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
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

fn extra_toolchain_dirs() -> Vec<PathBuf> {
    match std::env::var_os("LSW_TOOLCHAIN_DIRS") {
        Some(v) => std::env::split_paths(&v)
            .filter(|d| !d.as_os_str().is_empty())
            .collect(),
        None => Vec::new(),
    }
}

pub(crate) fn derive_sysroot(cc: &Path, triple: &str) -> PathBuf {
    if let Some(bindir) = cc.parent()
        && let Some(root) = bindir.parent()
    {
        let candidate = root.join(triple);
        if candidate.join("include").join("windows.h").is_file()
            || candidate.join("include").join("Windows.h").is_file()
        {
            return candidate;
        }
    }
    PathBuf::from(format!("/usr/{triple}"))
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

pub(crate) fn run_tool(
    tool: &Path,
    configure: impl FnOnce(&mut Command),
) -> Result<String, String> {
    let mut cmd = Command::new(tool);
    configure(&mut cmd);
    match capped_output(&mut cmd) {
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
