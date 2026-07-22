use std::path::{Path, PathBuf};

use lsw_config::{ResolvedToolchain, TargetArch};

use crate::error::{LLVM_MINGW_ID, MINGW_GCC_ID, ToolchainError};
use crate::provider::{ToolchainProvider, unavailable};
use crate::util::{compiler_version, derive_sysroot, which};

pub struct LlvmMingw;

impl ToolchainProvider for LlvmMingw {
    fn id(&self) -> &'static str {
        LLVM_MINGW_ID
    }

    fn resolve(&self, arch: TargetArch) -> Result<ResolvedToolchain, ToolchainError> {
        let triple = arch.mingw_triple();
        let cc = which("clang").ok_or_else(|| unavailable(self.id(), "'clang' not on PATH"))?;
        let cxx =
            which("clang++").ok_or_else(|| unavailable(self.id(), "'clang++' not on PATH"))?;
        let sysroot = derive_sysroot(&cc, triple);
        if !sysroot.is_dir() {
            return Err(unavailable(
                self.id(),
                &format!("mingw-w64 sysroot {} does not exist", sysroot.display()),
            ));
        }
        let include = sysroot.join("include");
        if !include.join("windows.h").is_file() && !include.join("Windows.h").is_file() {
            return Err(unavailable(
                self.id(),
                &format!(
                    "{} has no windows.h; install the mingw-w64 headers",
                    include.display()
                ),
            ));
        }
        let mut link_flags = vec!["-fuse-ld=lld".to_owned()];
        if let Some(libgcc) = latest_gcc_lib_dir(triple) {
            link_flags.push(format!("-L{}", libgcc.display()));
        }
        link_flags.push("-static".to_owned());
        link_flags.push("-lwinpthread".to_owned());
        let cxx_flags = libstdcxx_include_dirs(&sysroot, triple)
            .into_iter()
            .map(|d| format!("-I{}", d.display()))
            .collect();
        Ok(ResolvedToolchain {
            provider: self.id().to_owned(),
            version: compiler_version(&cc),
            c_flags: vec![
                format!("--target={triple}"),
                format!("--sysroot={}", sysroot.display()),
            ],
            cxx_flags,
            link_flags,
            cc,
            cxx,
            sysroot,
        })
    }
}

fn libstdcxx_include_dirs(sysroot: &Path, triple: &str) -> Vec<PathBuf> {
    let base = sysroot.join("include/c++");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("iostream").is_file())
        .collect();
    versions.sort();
    match versions.pop() {
        Some(dir) => {
            let target = dir.join(triple);
            let mut out = vec![dir];
            if target.is_dir() {
                out.push(target);
            }
            out
        }
        None => Vec::new(),
    }
}

fn latest_gcc_lib_dir(triple: &str) -> Option<PathBuf> {
    let base = PathBuf::from("/usr/lib/gcc").join(triple);
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("libgcc.a").is_file())
        .collect();
    versions.sort();
    versions.pop()
}

pub struct MingwGcc;

impl ToolchainProvider for MingwGcc {
    fn id(&self) -> &'static str {
        MINGW_GCC_ID
    }

    fn resolve(&self, arch: TargetArch) -> Result<ResolvedToolchain, ToolchainError> {
        let triple = arch.mingw_triple();
        let gcc = format!("{triple}-gcc");
        let gxx = format!("{triple}-g++");
        let cc =
            which(&gcc).ok_or_else(|| unavailable(self.id(), &format!("'{gcc}' not on PATH")))?;
        let cxx =
            which(&gxx).ok_or_else(|| unavailable(self.id(), &format!("'{gxx}' not on PATH")))?;
        let sysroot = derive_sysroot(&cc, triple);
        Ok(ResolvedToolchain {
            provider: self.id().to_owned(),
            version: compiler_version(&cc),
            c_flags: Vec::new(),
            cxx_flags: Vec::new(),
            link_flags: vec!["-static".to_owned()],
            cc,
            cxx,
            sysroot,
        })
    }
}
