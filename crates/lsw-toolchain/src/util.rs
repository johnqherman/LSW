use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const MAX_TOOL_OUTPUT: u64 = 16 * 1024 * 1024;
const DRAIN_WAIT: Duration = Duration::from_secs(5);

pub struct CappedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

pub fn drain_capped(
    mut reader: impl std::io::Read + Send + 'static,
    cap: u64,
) -> mpsc::Receiver<Vec<u8>> {
    use std::io::Read;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = reader.by_ref().take(cap).read_to_end(&mut b);
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
        let _ = tx.send(b);
    });
    rx
}

pub fn capped_output_with(
    cmd: &mut Command,
    cap: u64,
    timeout: Option<Duration>,
) -> std::io::Result<CappedOutput> {
    use std::process::Stdio;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let rx_out = drain_capped(child.stdout.take().expect("piped stdout"), cap);
    let rx_err = drain_capped(child.stderr.take().expect("piped stderr"), cap);
    let mut timed_out = false;
    let status = match timeout {
        None => child.wait()?,
        Some(limit) => {
            let deadline = Instant::now() + limit;
            loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait()?;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
    let stdout = rx_out.recv_timeout(DRAIN_WAIT).unwrap_or_default();
    let stderr = rx_err.recv_timeout(DRAIN_WAIT).unwrap_or_default();
    Ok(CappedOutput {
        status,
        stdout,
        stderr,
        timed_out,
    })
}

pub(crate) fn capped_output(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    let out = capped_output_with(cmd, MAX_TOOL_OUTPUT, None)?;
    Ok(std::process::Output {
        status: out.status,
        stdout: out.stdout,
        stderr: out.stderr,
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

pub(crate) fn starts_with_mz(path: &Path) -> bool {
    use std::io::Read as _;
    std::fs::File::open(path)
        .ok()
        .and_then(|mut f| {
            let mut magic = [0u8; 2];
            f.read_exact(&mut magic).ok().map(|_| magic)
        })
        .is_some_and(|m| &m == b"MZ")
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
