use std::path::PathBuf;

use clap::CommandFactory;

use crate::cli::Cli;

pub(crate) struct InstallPaths {
    pub(crate) bash: PathBuf,
    pub(crate) zsh: PathBuf,
    pub(crate) fish: PathBuf,
    pub(crate) man: PathBuf,
}

pub(crate) fn install_paths(prefix: &std::path::Path) -> InstallPaths {
    InstallPaths {
        bash: prefix.join("share/bash-completion/completions"),
        zsh: prefix.join("share/zsh/site-functions"),
        fish: prefix.join("share/fish/vendor_completions.d"),
        man: prefix.join("share/man/man1"),
    }
}

pub(crate) fn default_prefix() -> PathBuf {
    if let Some(p) = std::env::var_os("PREFIX") {
        return PathBuf::from(p);
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local"))
        .unwrap_or_else(|| PathBuf::from("/usr/local"))
}

fn write_completion(
    shell: clap_complete::Shell,
    cmd: &mut clap::Command,
    path: &std::path::Path,
) -> lsw_core::Result<()> {
    let mut file =
        std::fs::File::create(path).map_err(|e| lsw_core::Error::io(path.to_path_buf(), e))?;
    clap_complete::generate(shell, cmd, "lsw", &mut file);
    Ok(())
}

pub(crate) fn run_install(prefix: &std::path::Path) -> lsw_core::Result<()> {
    let p = install_paths(prefix);
    for dir in [&p.bash, &p.zsh, &p.fish, &p.man] {
        std::fs::create_dir_all(dir).map_err(|e| lsw_core::Error::io(dir.clone(), e))?;
    }

    let mut cmd = Cli::command();
    write_completion(clap_complete::Shell::Bash, &mut cmd, &p.bash.join("lsw"))?;
    write_completion(clap_complete::Shell::Zsh, &mut cmd, &p.zsh.join("_lsw"))?;
    write_completion(
        clap_complete::Shell::Fish,
        &mut cmd,
        &p.fish.join("lsw.fish"),
    )?;
    println!("installed completions under {}", p.bash.display());

    write_man_page(&cmd, &p.man, "lsw")?;
    for sub in cmd.get_subcommands() {
        write_man_page(sub, &p.man, &format!("lsw-{}", sub.get_name()))?;
    }
    println!("installed man pages under {}", p.man.display());
    Ok(())
}

pub(crate) fn write_man_page(
    cmd: &clap::Command,
    dir: &std::path::Path,
    name: &str,
) -> lsw_core::Result<()> {
    let path = dir.join(format!("{name}.1"));
    let mut buf = Vec::new();
    clap_mangen::Man::new(cmd.clone())
        .title(name.to_uppercase())
        .render(&mut buf)
        .map_err(|e| lsw_core::Error::io(path.clone(), e))?;
    std::fs::write(&path, buf).map_err(|e| lsw_core::Error::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_paths_follow_xdg_layout() {
        let p = install_paths(std::path::Path::new("/home/u/.local"));
        assert!(p.bash.ends_with("bash-completion/completions"));
        assert!(p.zsh.ends_with("zsh/site-functions"));
        assert!(p.fish.ends_with("fish/vendor_completions.d"));
        assert_eq!(p.man, PathBuf::from("/home/u/.local/share/man/man1"));
    }

    #[test]
    fn default_prefix_honors_prefix_env() {
        unsafe { std::env::set_var("PREFIX", "/opt/lsw") };
        assert_eq!(default_prefix(), PathBuf::from("/opt/lsw"));
        unsafe { std::env::remove_var("PREFIX") };
    }
}
