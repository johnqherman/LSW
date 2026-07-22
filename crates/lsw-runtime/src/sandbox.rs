use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use crate::types::{NetworkMode, SandboxSpec};

pub fn find_pasta() -> Option<PathBuf> {
    find_on_path("pasta").or_else(|| find_on_path("slirp4netns"))
}

pub(crate) fn should_unshare_net(mode: NetworkMode, pasta_available: bool) -> bool {
    match mode {
        NetworkMode::Host => false,
        NetworkMode::None => true,
        NetworkMode::Isolated => !pasta_available,
    }
}

pub fn bwrap_args(spec: &SandboxSpec, unshare_net: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--die-with-parent",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/etc",
        "/etc",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib64",
        "/lib64",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/bin",
        "/sbin",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-ipc",
        "--new-session",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    if let Some(home) = dirs_home() {
        args.push("--tmpfs".into());
        args.push(home.display().to_string());
    }
    for path in &spec.rw_binds {
        let p = path.display().to_string();
        args.push("--bind".into());
        args.push(p.clone());
        args.push(p);
    }
    if unshare_net {
        args.push("--unshare-net".into());
    }
    args
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub(crate) fn apply_rlimits(command: &mut Command, spec: &SandboxSpec) {
    let cpu = spec.cpu_seconds;
    let mem = spec.memory_bytes;
    if cpu.is_none() && mem.is_none() {
        return;
    }
    unsafe {
        command.pre_exec(move || {
            if let Some(secs) = cpu {
                let lim = libc::rlimit {
                    rlim_cur: secs,
                    rlim_max: secs,
                };
                libc::setrlimit(libc::RLIMIT_CPU, &lim);
            }
            if let Some(bytes) = mem {
                let lim = libc::rlimit {
                    rlim_cur: bytes,
                    rlim_max: bytes,
                };
                libc::setrlimit(libc::RLIMIT_AS, &lim);
            }
            Ok(())
        });
    }
}

pub(crate) fn sandbox_base_env() -> Vec<(String, String)> {
    const ALLOW: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "TERM",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "DISPLAY",
        "XAUTHORITY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
    ];
    let mut out = Vec::new();
    for key in ALLOW {
        if let Some(value) = std::env::var_os(key)
            && let Ok(value) = value.into_string()
        {
            out.push(((*key).to_owned(), value));
        }
    }
    if !out.iter().any(|(k, _)| k == "PATH") {
        out.push(("PATH".to_owned(), "/usr/bin:/bin".to_owned()));
    }
    out
}

pub fn find_bwrap() -> Option<PathBuf> {
    find_on_path("bwrap")
}

pub fn find_xvfb_run() -> Option<PathBuf> {
    find_on_path("xvfb-run")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|c| c.is_file())
}
