#![allow(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

use criterion::{Criterion, criterion_group, criterion_main};

const MINGW_GCC: &str = "x86_64-w64-mingw32-gcc";

fn build_fixture() -> Option<(tempfile::TempDir, PathBuf)> {
    if Command::new(MINGW_GCC).arg("--version").output().is_err() {
        return None;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("tick.c");
    std::fs::write(
        &src,
        b"#include <windows.h>\nint main(void) { return (int)(GetTickCount() & 1); }\n",
    )
    .expect("write src");
    let exe = dir.path().join("tick.exe");
    let out = Command::new(MINGW_GCC)
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("spawn gcc");
    if !out.status.success() {
        return None;
    }
    Some((dir, exe))
}

fn bench_deps_tree(c: &mut Criterion) {
    let Some((_dir, exe)) = build_fixture() else {
        eprintln!("skipping deps_tree benchmarks: {MINGW_GCC} not found");
        return;
    };

    c.bench_function("tree_with_dirs (no sysroot)", |b| {
        let dirs: Vec<PathBuf> = vec![exe.parent().unwrap().to_path_buf()];
        b.iter(|| lsw_core::depsops::tree_with_dirs(&dirs, &exe).unwrap());
    });
}

criterion_group!(benches, bench_deps_tree);
criterion_main!(benches);
