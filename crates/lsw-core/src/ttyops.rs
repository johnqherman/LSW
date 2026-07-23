use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

const CTRL_C: u8 = 0x03;
const DOUBLE_PRESS_WINDOW: Duration = Duration::from_secs(2);

static WINCH_PENDING: AtomicBool = AtomicBool::new(false);
static TERMIOS_RESTORE: AtomicPtr<libc::termios> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn on_winch(_: libc::c_int) {
    WINCH_PENDING.store(true, Ordering::Relaxed);
}

extern "C" fn on_fatal(sig: libc::c_int) {
    let saved = TERMIOS_RESTORE.load(Ordering::Acquire);
    if !saved.is_null() {
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, saved);
        }
    }
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

pub fn stdin_is_tty() -> bool {
    unsafe { libc::isatty(0) == 1 }
}

struct RawModeGuard {
    original: libc::termios,
    prev_term: libc::sigaction,
    prev_hup: libc::sigaction,
}

impl RawModeGuard {
    fn install() -> Option<Self> {
        unsafe {
            let mut original: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut original) != 0 {
                return None;
            }
            let saved = Box::into_raw(Box::new(original));
            TERMIOS_RESTORE.store(saved, Ordering::Release);
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_fatal as extern "C" fn(libc::c_int) as usize;
            let mut prev_term: libc::sigaction = std::mem::zeroed();
            let mut prev_hup: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGTERM, &action, &mut prev_term);
            libc::sigaction(libc::SIGHUP, &action, &mut prev_hup);
            let mut raw = original;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(0, libc::TCSANOW, &raw) != 0 {
                libc::sigaction(libc::SIGTERM, &prev_term, std::ptr::null_mut());
                libc::sigaction(libc::SIGHUP, &prev_hup, std::ptr::null_mut());
                let saved = TERMIOS_RESTORE.swap(std::ptr::null_mut(), Ordering::AcqRel);
                if !saved.is_null() {
                    drop(Box::from_raw(saved));
                }
                return None;
            }
            Some(RawModeGuard {
                original,
                prev_term,
                prev_hup,
            })
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.original);
            libc::sigaction(libc::SIGTERM, &self.prev_term, std::ptr::null_mut());
            libc::sigaction(libc::SIGHUP, &self.prev_hup, std::ptr::null_mut());
            let saved = TERMIOS_RESTORE.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !saved.is_null() {
                drop(Box::from_raw(saved));
            }
        }
    }
}

struct WinchGuard {
    previous: libc::sigaction,
}

impl WinchGuard {
    fn install() -> Self {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_winch as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGWINCH, &action, &mut previous);
            WinchGuard { previous }
        }
    }
}

impl Drop for WinchGuard {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGWINCH, &self.previous, std::ptr::null_mut());
        }
    }
}

fn open_pty() -> Result<(OwnedFd, OwnedFd)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if rc != 0 {
        return Err(Error::io("/dev/ptmx", std::io::Error::last_os_error()));
    }
    unsafe { Ok((OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave))) }
}

fn copy_winsize(master: &OwnedFd) {
    use std::os::fd::AsRawFd;
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        if libc::ioctl(0, libc::TIOCGWINSZ, &mut size) == 0 {
            libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size);
        }
    }
}

fn read_fd(fd: RawFd, buf: &mut [u8]) -> isize {
    unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) }
}

fn write_all_fd(fd: RawFd, mut data: &[u8]) {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if n <= 0 {
            return;
        }
        data = &data[n as usize..];
    }
}

pub fn run_shell_in_pty(mut command: Command, exit_hint: &str) -> Result<ExitStatus> {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    let (master, slave) = open_pty()?;
    copy_winsize(&master);

    let slave_stdin = slave.try_clone().map_err(|e| Error::io("/dev/pts", e))?;
    let slave_stdout = slave.try_clone().map_err(|e| Error::io("/dev/pts", e))?;
    command
        .stdin(Stdio::from(slave_stdin))
        .stdout(Stdio::from(slave_stdout))
        .stderr(Stdio::from(slave));
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }

    let program = command.get_program().to_owned();
    let mut child = command
        .spawn()
        .map_err(|e| Error::io(std::path::PathBuf::from(&program), e))?;
    drop(command);

    let _winch = WinchGuard::install();
    let _raw = RawModeGuard::install();

    let master_fd = master.as_raw_fd();
    let mut last_ctrl_c: Option<Instant> = None;
    let mut killed = false;
    let mut buf = [0u8; 4096];

    loop {
        if WINCH_PENDING.swap(false, Ordering::Relaxed) {
            copy_winsize(&master);
        }
        let mut fds = [
            libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, 100) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }

        if fds[0].revents & libc::POLLIN != 0 {
            let n = read_fd(0, &mut buf);
            if n <= 0 {
                break;
            }
            let input = &buf[..n as usize];
            for &byte in input {
                if byte != CTRL_C {
                    continue;
                }
                let now = Instant::now();
                if let Some(prev) = last_ctrl_c
                    && now.duration_since(prev) <= DOUBLE_PRESS_WINDOW
                {
                    let _ = child.kill();
                    killed = true;
                } else {
                    write_all_fd(2, exit_hint.as_bytes());
                }
                last_ctrl_c = Some(now);
            }
            if !killed {
                write_all_fd(master_fd, input);
            }
        }

        if fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let n = read_fd(master_fd, &mut buf);
            if n <= 0 {
                break;
            }
            write_all_fd(1, &buf[..n as usize]);
        }

        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
    }

    loop {
        let mut fds = [libc::pollfd {
            fd: master_fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        if unsafe { libc::poll(fds.as_mut_ptr(), 1, 0) } <= 0 || fds[0].revents & libc::POLLIN == 0
        {
            break;
        }
        let n = read_fd(master_fd, &mut buf);
        if n <= 0 {
            break;
        }
        write_all_fd(1, &buf[..n as usize]);
    }

    child
        .wait()
        .map_err(|e| Error::io(std::path::PathBuf::from(program), e))
}
