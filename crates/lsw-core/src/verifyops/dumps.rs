use std::process::Command;

use super::ssh_opts;

pub(crate) fn newest_dump(
    host: &str,
    identity: Option<&str>,
    dump_remote: &str,
    exe: &str,
) -> Option<String> {
    let out = super::capped_output(
        Command::new("ssh")
            .args(ssh_opts(identity))
            .arg(host)
            .arg(format!("cmd /c dir /b /o-d \"{dump_remote}\\{exe}.*.dmp\"")),
    )
    .ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let t = l.trim();
        (!t.is_empty() && t.to_ascii_lowercase().ends_with(".dmp")).then(|| t.to_owned())
    })
}

pub(crate) fn collect_dump(
    host: &str,
    identity: Option<&str>,
    dump_remote: &str,
    exe: &str,
    before: Option<&str>,
    dump_local: &std::path::Path,
) -> Option<String> {
    let mut name = None;
    for attempt in 0..8 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        if let Some(found) = newest_dump(host, identity, dump_remote, exe)
            && before != Some(found.as_str())
        {
            name = Some(found);
            break;
        }
    }
    let name = name?;
    std::fs::create_dir_all(dump_local).ok()?;
    let dest = dump_local.join(&name);
    let remote_fwd = dump_remote.replace('\\', "/");
    let scp = super::capped_output(
        Command::new("scp")
            .args(ssh_opts(identity))
            .arg(format!("{host}:{remote_fwd}/{name}"))
            .arg(&dest),
    )
    .ok()?;
    scp.status.success().then(|| dest.display().to_string())
}
