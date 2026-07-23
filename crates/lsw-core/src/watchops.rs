use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use crate::buildops::{self, BuildOptions};
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

const IGNORED_TOP_DIRS: &[&str] = &["build", "target", ".git", "dist"];

fn is_source_change(paths: &[PathBuf], root: &Path, outputs: &[PathBuf]) -> bool {
    paths.iter().any(|p| {
        let rel = p.strip_prefix(root).unwrap_or(p);
        let first = rel.components().next().and_then(|c| c.as_os_str().to_str());
        if first.is_some_and(|f| IGNORED_TOP_DIRS.contains(&f)) {
            return false;
        }
        !outputs.iter().any(|o| o == p)
    })
}

fn notify_err(root: &Path, e: notify::Error) -> Error {
    Error::io(root.to_path_buf(), std::io::Error::other(e))
}

fn rebuild(project: &Project, env: &Environment) -> Option<Vec<PathBuf>> {
    let opts = BuildOptions {
        system: None,
        update_lock: false,
        reproducible: false,
        aot: false,
    };
    match buildops::build(project, env, &opts) {
        Ok(report) => {
            println!("[watch] build ok: {} artifact(s)", report.artifacts.len());
            Some(
                report
                    .artifacts
                    .iter()
                    .map(|a| project.root.join(a))
                    .collect(),
            )
        }
        Err(e) => {
            eprintln!("[watch] build failed: {e}");
            None
        }
    }
}

pub fn watch(project: &Project, env: &Environment) -> Result<()> {
    println!(
        "[watch] watching {} (Ctrl-C to stop)",
        project.root.display()
    );
    let mut outputs = rebuild(project, env).unwrap_or_default();

    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| notify_err(&project.root, e))?;
    watcher
        .watch(&project.root, RecursiveMode::Recursive)
        .map_err(|e| notify_err(&project.root, e))?;

    loop {
        let first = rx
            .recv()
            .map_err(|_| Error::io(project.root.clone(), std::io::Error::other("watch ended")))?;
        let mut paths = event_paths(first);
        while let Ok(next) = rx.recv_timeout(Duration::from_millis(300)) {
            paths.extend(event_paths(next));
        }
        if is_source_change(&paths, &project.root, &outputs)
            && let Some(next) = rebuild(project, env)
        {
            outputs = next;
            while rx.recv_timeout(Duration::from_millis(500)).is_ok() {}
        }
    }
}

fn event_paths(res: notify::Result<notify::Event>) -> Vec<PathBuf> {
    res.map(|e| e.paths).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_source_change_ignores_build_outputs() {
        let root = Path::new("/proj");
        let outputs = vec![
            PathBuf::from("/proj/app.exe"),
            PathBuf::from("/proj/foo.dll"),
        ];
        assert!(is_source_change(
            &[PathBuf::from("/proj/src/main.c")],
            root,
            &outputs
        ));
        assert!(!is_source_change(
            &[
                PathBuf::from("/proj/build/main.exe"),
                PathBuf::from("/proj/target/x/app.exe"),
            ],
            root,
            &outputs
        ));
        assert!(is_source_change(
            &[
                PathBuf::from("/proj/build/x.o"),
                PathBuf::from("/proj/CMakeLists.txt"),
            ],
            root,
            &outputs
        ));
        assert!(!is_source_change(
            &[
                PathBuf::from("/proj/app.exe"),
                PathBuf::from("/proj/foo.dll")
            ],
            root,
            &outputs
        ));
        assert!(is_source_change(
            &[PathBuf::from("/proj/vendor/foo.lib")],
            root,
            &outputs
        ));
    }
}
