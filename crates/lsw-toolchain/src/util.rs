use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const MAX_TOOL_OUTPUT: u64 = 16 * 1024 * 1024;
const DRAIN_WAIT: Duration = Duration::from_secs(5);

/// Output of a subprocess with capped stdout/stderr and optional timeout.
pub struct CappedOutput {
    /// Process exit status.
    pub status: ExitStatus,
    /// Captured stdout (truncated at the cap).
    pub stdout: Vec<u8>,
    /// Captured stderr (truncated at the cap).
    pub stderr: Vec<u8>,
    /// Whether the process was killed due to timeout.
    pub timed_out: bool,
}

/// Handle to a background thread draining a reader into a bounded buffer.
pub struct Drain {
    done: mpsc::Receiver<()>,
    buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl Drain {
    /// Waits for EOF and returns the accumulated buffer.
    pub fn wait_eof(self) -> Vec<u8> {
        let _ = self.done.recv();
        self.take()
    }

    /// Waits up to `timeout` for EOF and returns the accumulated buffer.
    pub fn wait_timeout(self, timeout: Duration) -> Vec<u8> {
        let _ = self.done.recv_timeout(timeout);
        self.take()
    }

    fn take(self) -> Vec<u8> {
        let mut guard = self
            .buf
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *guard)
    }
}

/// Spawns a thread that reads `reader` into a bounded buffer of at most `cap` bytes.
pub fn drain_capped(mut reader: impl std::io::Read + Send + 'static, cap: u64) -> Drain {
    let (tx, done) = mpsc::channel();
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = std::sync::Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        let mut kept = 0u64;
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if kept < cap {
                        let take = usize::try_from(cap - kept).unwrap_or(usize::MAX).min(n);
                        let mut guard = shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        guard.extend_from_slice(&chunk[..take]);
                        kept += take as u64;
                    }
                }
            }
        }
        let _ = tx.send(());
    });
    Drain { done, buf }
}

/// Runs a command capturing stdout/stderr up to `cap` bytes with an optional timeout.
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
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => {}
                    Err(e) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(e);
                    }
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
    let stdout = rx_out.wait_timeout(DRAIN_WAIT);
    let stderr = rx_err.wait_timeout(DRAIN_WAIT);
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

/// Returns the first line of `cc --version`, or `"unknown"`.
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
            f.read_exact(&mut magic).ok().map(|()| magic)
        })
        .is_some_and(|m| &m == b"MZ")
}

/// Returns the hex-encoded SHA-256 digest of a file.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Returns the hex-encoded SHA-256 digest of a byte slice.
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

/// Locates the `windres` resource compiler next to `cc` or on PATH.
pub fn find_windres(cc: &Path, triple: &str) -> Option<PathBuf> {
    let triple_tool = format!("{triple}-windres");
    if let Some(bindir) = cc.parent() {
        for name in [triple_tool.as_str(), "llvm-windres"] {
            let candidate = bindir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    which(&triple_tool).or_else(|| which("llvm-windres"))
}

fn extra_toolchain_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = match std::env::var_os("LSW_TOOLCHAIN_DIRS") {
        Some(v) => std::env::split_paths(&v)
            .filter(|d| !d.as_os_str().is_empty())
            .collect(),
        None => Vec::new(),
    };
    dirs.extend(managed_toolchain_dirs());
    dirs
}

fn managed_toolchain_dirs() -> Vec<PathBuf> {
    let Ok(base) = lsw_config::Dirs::resolve() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(base.toolchains()) else {
        return Vec::new();
    };
    let mut named: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let bin = e.path().join("bin");
            bin.is_dir()
                .then(|| (e.file_name().to_string_lossy().into_owned(), bin))
        })
        .collect();
    named.sort_by(|a, b| b.0.cmp(&a.0));
    named.into_iter().map(|(_, p)| p).collect()
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
