use std::path::Path;
use std::process::Command;

pub(crate) fn process_in_prefix(pid: u32, prefix: &Path) -> bool {
    let Ok(environ) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    let needle = format!("WINEPREFIX={}", prefix.display());
    environ
        .split(|b| *b == 0)
        .any(|entry| entry == needle.as_bytes())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn pidfd_open(pid: u32) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0i32) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) })
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn pidfd_send_signal(pidfd: &std::os::fd::OwnedFd, sig: i32) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let rc = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            sig,
            std::ptr::null::<libc::siginfo_t>(),
            0i32,
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn kill_validated(pid: u32, prefix: &Path) -> Result<(), crate::RuntimeError> {
    #[cfg(target_os = "linux")]
    if let Ok(pidfd) = pidfd_open(pid) {
        if !process_in_prefix(pid, prefix) {
            return Err(crate::RuntimeError::ProcessNotInEnvironment { pid });
        }
        return pidfd_send_signal(&pidfd, libc::SIGTERM)
            .map_err(|_| crate::RuntimeError::ProcessNotInEnvironment { pid });
    }
    if !process_in_prefix(pid, prefix) {
        return Err(crate::RuntimeError::ProcessNotInEnvironment { pid });
    }
    #[allow(unsafe_code)]
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if rc != 0 {
        return Err(crate::RuntimeError::ProcessNotInEnvironment { pid });
    }
    Ok(())
}

const HOST_WINE_VARS: &[&str] = &[
    "WINEPREFIX",
    "WINEARCH",
    "WINEPATH",
    "WINEDLLPATH",
    "WINEDLLOVERRIDES",
    "WINESERVER",
    "WINELOADER",
    "WINEDEBUG",
    "WINEFSYNC",
    "WINEESYNC",
];

/// Removes host-side Wine environment variables from a command.
pub fn scrub_wine_env(command: &mut Command) {
    for var in HOST_WINE_VARS {
        command.env_remove(var);
    }
}
