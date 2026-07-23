use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use lsw_config::{ResolvedToolchain, TargetArch};

use crate::buildops::which;
use crate::envops::Environment;
use crate::error::{Error, Result};
use crate::project::Project;

fn sh_squote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

const GLUE_SOURCE: &str = r#"extern "C" {
typedef unsigned long long uptr;
typedef decltype(sizeof(0)) usize;
void* malloc(usize);
void free(void*);
__declspec(dllimport) __declspec(noreturn) void ExitProcess(unsigned int);
__declspec(dllimport) wchar_t* GetCommandLineW(void);
__declspec(dllimport) wchar_t** CommandLineToArgvW(const wchar_t*, int*);
int wmain(int argc, wchar_t** argv);
#pragma comment(lib, "shell32.lib")

int _fltused = 1;
unsigned long long __security_cookie = 0x00002B992DDFA232ULL;
void __security_check_cookie(uptr) {}
int __GSHandlerCheck(void*, void*, void*, void*) { return 1; }
unsigned int _tls_index = 0;

typedef void (*atexit_fn)(void);
static atexit_fn atexit_table[64];
static int atexit_count = 0;
int atexit(atexit_fn fn) {
    if (atexit_count >= 64) return 1;
    atexit_table[atexit_count++] = fn;
    return 0;
}

void wmainCRTStartup(void) {
    int argc = 0;
    wchar_t** argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    int code = wmain(argc, argv);
    while (atexit_count > 0) atexit_table[--atexit_count]();
    ExitProcess((unsigned int)code);
}

#pragma section(".tls", read, write)
#pragma section(".tls$ZZZ", read, write)
#pragma section(".CRT$XLA", read)
#pragma section(".CRT$XLZ", read)
#pragma section(".rdata$T", read)

__declspec(allocate(".tls")) char _tls_start = 0;
__declspec(allocate(".tls$ZZZ")) char _tls_end = 0;

typedef void(__stdcall* tls_callback)(void*, unsigned long, void*);
__declspec(allocate(".CRT$XLA")) tls_callback __xl_a = 0;
__declspec(allocate(".CRT$XLZ")) tls_callback __xl_z = 0;

struct tls_directory {
    uptr start;
    uptr end;
    uptr index;
    uptr callbacks;
    unsigned int zero_fill;
    unsigned int characteristics;
};

__declspec(allocate(".rdata$T")) extern const tls_directory _tls_used = {
    (uptr)&_tls_start, (uptr)&_tls_end,  (uptr)&_tls_index,
    (uptr)(&__xl_a + 1), 0, 0,
};
}

__asm__(
    ".globl __chkstk\n"
    "__chkstk:\n"
    "  pushq %rcx\n"
    "  pushq %rax\n"
    "  cmpq $0x1000, %rax\n"
    "  leaq 24(%rsp), %rcx\n"
    "  jb 1f\n"
    "2:\n"
    "  subq $0x1000, %rcx\n"
    "  orl $0, (%rcx)\n"
    "  subq $0x1000, %rax\n"
    "  cmpq $0x1000, %rax\n"
    "  ja 2b\n"
    "1:\n"
    "  subq %rax, %rcx\n"
    "  orl $0, (%rcx)\n"
    "  popq %rax\n"
    "  popq %rcx\n"
    "  retq\n");

namespace std {
struct nothrow_t {};
extern const nothrow_t nothrow;
const nothrow_t nothrow;
}

void* operator new(usize n, const std::nothrow_t&) noexcept { return malloc(n); }
void* operator new[](usize n, const std::nothrow_t&) noexcept { return malloc(n); }
void operator delete(void* p) noexcept { free(p); }
void operator delete(void* p, usize) noexcept { free(p); }
void operator delete[](void* p) noexcept { free(p); }
void operator delete[](void* p, usize) noexcept { free(p); }
"#;

const IMPORT_LIBS: &[(&str, &str)] = &[
    ("advapi32.lib", "libadvapi32.a"),
    ("bcrypt.lib", "libbcrypt.a"),
    ("crypt32.lib", "libcrypt32.a"),
    ("iphlpapi.lib", "libiphlpapi.a"),
    ("kernel32.lib", "libkernel32.a"),
    ("mswsock.lib", "libmswsock.a"),
    ("ncrypt.lib", "libncrypt.a"),
    ("normaliz.lib", "libnormaliz.a"),
    ("ntdll.lib", "libntdll.a"),
    ("ole32.lib", "libole32.a"),
    ("oleaut32.lib", "liboleaut32.a"),
    ("secur32.lib", "libsecur32.a"),
    ("shell32.lib", "libshell32.a"),
    ("user32.lib", "libuser32.a"),
    ("uuid.lib", "libuuid.a"),
    ("version.lib", "libversion.a"),
    ("ws2_32.lib", "libws2_32.a"),
    ("ucrt.lib", "libucrt.a"),
    ("LIBCMT.lib", "libucrt.a"),
    ("libcpmt.lib", "libucrt.a"),
    ("OLDNAMES.lib", "libmoldname.a"),
    ("mingwex.lib", "libmingwex.a"),
    ("mingwcrt.lib", "libmingw32.a"),
];

pub struct AotSetup {
    pub linker_wrapper: PathBuf,
}

pub fn prepare(project: &Project, env: &Environment, tc: &ResolvedToolchain) -> Result<AotSetup> {
    if env.manifest.target_arch != TargetArch::X86_64 {
        return Err(Error::AotUnsupported {
            detail: format!(
                "NativeAOT cross-compilation currently targets x86_64 only, not {}",
                env.manifest.target_arch
            ),
        });
    }
    let lld_link = which("lld-link").ok_or(Error::ToolMissing {
        tool: "lld-link".into(),
        fix: "install lld; NativeAOT links PE binaries through lld-link".into(),
    })?;
    let clang = clang_compiler(tc)?;
    let lib_dir = sysroot_lib_dir(&tc.sysroot).ok_or_else(|| Error::AotUnsupported {
        detail: format!(
            "no mingw-w64 import libraries found under sysroot {}",
            tc.sysroot.display()
        ),
    })?;

    let aot_dir = project.root.join("build").join("lsw-aot");
    let shim_dir = aot_dir.join("libs");
    fs::create_dir_all(&shim_dir).map_err(|e| Error::io(shim_dir.clone(), e))?;

    for (msvc_name, mingw_name) in IMPORT_LIBS {
        let source = lib_dir.join(mingw_name);
        if !source.is_file() {
            return Err(Error::AotUnsupported {
                detail: format!(
                    "mingw-w64 import library {mingw_name} not found in {}",
                    lib_dir.display()
                ),
            });
        }
        let dest = shim_dir.join(msvc_name);
        if dest.symlink_metadata().is_ok() {
            fs::remove_file(&dest).map_err(|e| Error::io(dest.clone(), e))?;
        }
        std::os::unix::fs::symlink(&source, &dest).map_err(|e| Error::io(dest.clone(), e))?;
    }

    let glue_src = aot_dir.join("glue.cpp");
    fs::write(&glue_src, GLUE_SOURCE).map_err(|e| Error::io(glue_src.clone(), e))?;
    let glue_obj = aot_dir.join("glue.obj");
    let output = Command::new(&clang)
        .args([
            "--target=x86_64-pc-windows-msvc",
            "-fno-exceptions",
            "-O2",
            "-c",
        ])
        .arg(&glue_src)
        .arg("-o")
        .arg(&glue_obj)
        .output()
        .map_err(|e| Error::io(clang.clone(), e))?;
    if !output.status.success() {
        return Err(Error::BuildFailed {
            command: format!(
                "{} --target=x86_64-pc-windows-msvc -c glue.cpp: {}",
                clang.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            code: output.status.code(),
        });
    }

    let abs_shim = std::path::absolute(&shim_dir).map_err(|e| Error::io(shim_dir.clone(), e))?;
    let abs_obj = std::path::absolute(&glue_obj).map_err(|e| Error::io(glue_obj.clone(), e))?;
    let wrapper = aot_dir.join("lld-link.sh");
    let q_link = sh_squote(&lld_link.display().to_string());
    let q_libpath = sh_squote(&format!("/libpath:{}", abs_shim.display()));
    let q_obj = sh_squote(&abs_obj.display().to_string());
    let script = format!(
        "#!/bin/sh\n\
         newargs=\n\
         for a in \"$@\"; do\n\
         \x20 case \"$a\" in\n\
         \x20 @*)\n\
         \x20   rsp=\"${{a#@}}\"\n\
         \x20   sed -E 's#/(NOEXP|NOIMPLIB)([[:space:]]|$)#\\2#g' \"$rsp\" > \"$rsp.lsw\"\n\
         \x20   a=\"@$rsp.lsw\"\n\
         \x20   ;;\n\
         \x20 esac\n\
         \x20 newargs=\"$newargs '$(printf %s \"$a\" | sed \"s/'/'\\\\\\\\''/g\")'\"\n\
         done\n\
         eval set -- $newargs\n\
         exec {q_link} {q_libpath} {q_obj} \"$@\" \"mingwex.lib\" \"mingwcrt.lib\"\n",
    );
    fs::write(&wrapper, script).map_err(|e| Error::io(wrapper.clone(), e))?;
    let mut perms = fs::metadata(&wrapper)
        .map_err(|e| Error::io(wrapper.clone(), e))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&wrapper, perms).map_err(|e| Error::io(wrapper.clone(), e))?;

    let linker_wrapper =
        std::path::absolute(&wrapper).map_err(|e| Error::io(wrapper.clone(), e))?;
    Ok(AotSetup { linker_wrapper })
}

pub fn publish_args(setup: &AotSetup) -> Vec<String> {
    vec![
        "-p:PublishAot=true".to_owned(),
        "-p:DisableUnsupportedError=true".to_owned(),
        "-p:IlcUseEnvironmentalTools=true".to_owned(),
        format!("-p:CppLinker={}", setup.linker_wrapper.display()),
    ]
}

fn clang_compiler(tc: &ResolvedToolchain) -> Result<PathBuf> {
    let is_clang = tc
        .cc
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains("clang"));
    if is_clang {
        return Ok(tc.cc.clone());
    }
    which("clang").ok_or(Error::AotUnsupported {
        detail: "NativeAOT glue needs clang (MSVC-ABI codegen); install clang or use the llvm-mingw toolchain".into(),
    })
}

fn sysroot_lib_dir(sysroot: &Path) -> Option<PathBuf> {
    let candidates = [
        sysroot.join("lib"),
        sysroot.join("x86_64-w64-mingw32").join("lib"),
    ];
    candidates
        .into_iter()
        .find(|dir| dir.join("libkernel32.a").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_lib_table_covers_crt_aliases() {
        let names: Vec<&str> = IMPORT_LIBS.iter().map(|(m, _)| *m).collect();
        for required in [
            "LIBCMT.lib",
            "libcpmt.lib",
            "OLDNAMES.lib",
            "ucrt.lib",
            "shell32.lib",
        ] {
            assert!(names.contains(&required), "missing {required}");
        }
    }

    #[test]
    fn sysroot_lib_dir_finds_kernel32() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(sysroot_lib_dir(tmp.path()), None);
        let lib = tmp.path().join("x86_64-w64-mingw32").join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("libkernel32.a"), b"a").unwrap();
        assert_eq!(sysroot_lib_dir(tmp.path()), Some(lib));
    }

    #[test]
    fn publish_args_wire_the_wrapper() {
        let setup = AotSetup {
            linker_wrapper: PathBuf::from("/x/lld-link.sh"),
        };
        let args = publish_args(&setup);
        assert!(args.contains(&"-p:PublishAot=true".to_owned()));
        assert!(args.contains(&"-p:CppLinker=/x/lld-link.sh".to_owned()));
    }

    #[test]
    fn glue_source_defines_required_symbols() {
        for sym in [
            "__chkstk",
            "_fltused",
            "__security_cookie",
            "_tls_used",
            "wmainCRTStartup",
            "CommandLineToArgvW",
        ] {
            assert!(GLUE_SOURCE.contains(sym), "glue missing {sym}");
        }
    }
}
