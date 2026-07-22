use std::path::Path;
use std::process::Command;

pub fn process_in_prefix(pid: u32, prefix: &Path) -> bool {
    let Ok(environ) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    let needle = format!("WINEPREFIX={}", prefix.display());
    environ
        .split(|b| *b == 0)
        .any(|entry| entry == needle.as_bytes())
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

pub(crate) fn scrub_host_wine_vars(command: &mut Command) {
    for var in HOST_WINE_VARS {
        command.env_remove(var);
    }
}

pub fn scrub_wine_env(command: &mut Command) {
    scrub_host_wine_vars(command);
}
