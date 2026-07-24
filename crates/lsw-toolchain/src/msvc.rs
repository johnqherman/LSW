use std::path::{Path, PathBuf};
use std::process::Command;

use lsw_config::{ResolvedToolchain, TargetArch};

use crate::error::{CLANG_CL_ID, ProbeReport, ToolchainError};
use crate::provider::unavailable;
use crate::util::{compiler_version, which};

pub fn resolve_msvc(
    arch: TargetArch,
    sdk_root: &Path,
) -> Result<ResolvedToolchain, ToolchainError> {
    let cc = which("clang-cl")
        .ok_or_else(|| unavailable(CLANG_CL_ID, "'clang-cl' not on PATH (install clang)"))?;
    if which("lld-link").is_none() {
        return Err(unavailable(
            CLANG_CL_ID,
            "'lld-link' not on PATH (install lld); required as the MSVC-ABI linker",
        ));
    }
    if !sdk_root.is_dir() {
        return Err(unavailable(
            CLANG_CL_ID,
            &format!(
                "SDK sysroot {} does not exist; import one with 'lsw sdk import'",
                sdk_root.display()
            ),
        ));
    }

    let triple = arch.msvc_triple();
    let (includes, libs) = msvc_search_paths(sdk_root, arch.msvc_lib_dirs());
    if includes.is_empty() {
        return Err(unavailable(
            CLANG_CL_ID,
            &format!(
                "no MSVC/SDK include directories found under {}; expected an xwin (crt/ + sdk/) or flat (include/ + lib/) layout",
                sdk_root.display()
            ),
        ));
    }

    let mut c_flags = vec![format!("--target={triple}")];
    for inc in &includes {
        c_flags.push("-imsvc".to_owned());
        c_flags.push(inc.display().to_string());
    }
    let mut link_flags = vec!["-fuse-ld=lld-link".to_owned()];
    for lib in &libs {
        link_flags.push(format!("/libpath:{}", lib.display()));
    }

    Ok(ResolvedToolchain {
        provider: CLANG_CL_ID.to_owned(),
        version: compiler_version(&cc),
        c_flags,
        cxx_flags: Vec::new(),
        link_flags,
        cc: cc.clone(),
        cxx: cc,
        sysroot: sdk_root.to_path_buf(),
    })
}

pub(crate) fn msvc_search_paths(root: &Path, lib_archs: &[&str]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut includes = Vec::new();
    let mut libs = Vec::new();
    let push_if = |v: &mut Vec<PathBuf>, p: PathBuf| {
        if p.is_dir() {
            v.push(p);
        }
    };

    push_if(&mut includes, root.join("crt/include"));
    for comp in ["ucrt", "um", "shared", "winrt", "cppwinrt"] {
        push_if(&mut includes, root.join("sdk/include").join(comp));
    }
    for arch in lib_archs {
        push_if(&mut libs, root.join("crt/lib").join(arch));
        for comp in ["ucrt", "um"] {
            push_if(&mut libs, root.join("sdk/lib").join(comp).join(arch));
        }
    }

    push_if(&mut includes, root.join("include"));
    for arch in lib_archs {
        push_if(&mut libs, root.join("lib").join(arch));
    }
    push_if(&mut libs, root.join("lib"));

    (includes, libs)
}

pub fn probe_msvc(tc: &ResolvedToolchain) -> ProbeReport {
    let fail = |detail: String, compiled: bool| ProbeReport {
        provider: CLANG_CL_ID.to_owned(),
        compiled,
        linked: false,
        produced_pe: false,
        detail,
    };

    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return fail(format!("cannot create temp dir: {e}"), false),
    };
    let src = dir.path().join("probe.c");
    let exe = dir.path().join("probe.exe");
    if std::fs::write(&src, "int main(void){return 0;}\n").is_err() {
        return fail("cannot write probe source".to_owned(), false);
    }

    let mut cmd = Command::new(&tc.cc);
    cmd.args(&tc.c_flags);
    if tc.link_flags.iter().any(|f| f == "-fuse-ld=lld-link") {
        cmd.arg("-fuse-ld=lld-link");
    }
    cmd.arg(&src)
        .arg(format!("/Fe{}", exe.display()))
        .arg("/link");
    for f in &tc.link_flags {
        if f.starts_with("-fuse-ld=") {
            continue;
        }
        cmd.arg(f);
    }

    match crate::util::capped_output(&mut cmd) {
        Ok(out) if out.status.success() && starts_with_mz(&exe) => ProbeReport {
            provider: CLANG_CL_ID.to_owned(),
            compiled: true,
            linked: true,
            produced_pe: true,
            detail: "clang-cl produced an MSVC-ABI PE against the imported SDK".to_owned(),
        },
        link_result => {
            let obj = dir.path().join("probe.obj");
            let mut compile = Command::new(&tc.cc);
            compile
                .args(&tc.c_flags)
                .arg("/c")
                .arg(&src)
                .arg(format!("/Fo{}", obj.display()));
            let compiled = crate::util::capped_output(&mut compile)
                .map(|o| o.status.success() && obj.is_file())
                .unwrap_or(false);
            let link_err = match link_result {
                Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                Err(e) => e.to_string(),
            };
            if compiled {
                fail(
                    format!(
                        "clang-cl compiles but could not link a PE (SDK import libraries incomplete?): {link_err}"
                    ),
                    true,
                )
            } else {
                fail(format!("clang-cl failed: {link_err}"), false)
            }
        }
    }
}

fn starts_with_mz(path: &Path) -> bool {
    use std::io::Read as _;
    std::fs::File::open(path)
        .ok()
        .and_then(|mut f| {
            let mut magic = [0u8; 2];
            f.read_exact(&mut magic).ok().map(|_| magic)
        })
        .map(|m| &m == b"MZ")
        .unwrap_or(false)
}
