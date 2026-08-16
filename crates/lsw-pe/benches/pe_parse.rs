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

fn bench_detect(c: &mut Criterion) {
    let Some((_dir, exe)) = build_fixture() else {
        eprintln!("skipping pe_parse benchmarks: {MINGW_GCC} not found");
        return;
    };

    c.bench_function("detect", |b| {
        b.iter(|| lsw_pe::detect(&exe).unwrap());
    });

    c.bench_function("open+info", |b| {
        b.iter(|| {
            let img = lsw_pe::PeImage::open(&exe).unwrap();
            img.info().unwrap()
        });
    });

    c.bench_function("open+imports", |b| {
        b.iter(|| {
            let img = lsw_pe::PeImage::open(&exe).unwrap();
            img.imports().unwrap()
        });
    });

    c.bench_function("open+hardening", |b| {
        b.iter(|| {
            let img = lsw_pe::PeImage::open(&exe).unwrap();
            img.hardening().unwrap()
        });
    });

    c.bench_function("open+all", |b| {
        b.iter(|| {
            let img = lsw_pe::PeImage::open(&exe).unwrap();
            let _ = img.info().unwrap();
            let _ = img.imports().unwrap();
            let _ = img.hardening().unwrap();
            let _ = img.details().unwrap();
            img.resources()
        });
    });
}

criterion_group!(benches, bench_detect);
criterion_main!(benches);
