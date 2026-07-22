use std::io::Read;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{
    ImageNtHeaders, ImageOptionalHeader, Import, PeFile, optional_header_magic,
};

#[derive(Debug, thiserror::Error)]
pub enum PeError {
    #[error(
        "LSW1301: cannot read {}: {source}; check that the file exists and is readable",
        path.display()
    )]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "LSW1302: {} has an MZ header but is not a valid PE image ({detail}); \
         the file is likely truncated or corrupted - rebuild it or restore it from source",
        path.display()
    )]
    MalformedPe { path: PathBuf, detail: String },
    #[error(
        "LSW1303: {} is not a PE executable; pass a Windows binary (.exe/.dll) \
         such as one produced by `lsw build`",
        path.display()
    )]
    NotPe { path: PathBuf },
}

impl PeError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        PeError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    fn malformed(path: &Path, detail: impl fmt::Display) -> Self {
        PeError::MalformedPe {
            path: path.to_path_buf(),
            detail: detail.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryKind {
    Pe(PeInfo),
    Elf,
    Script,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeInfo {
    pub format: PeFormat,
    pub machine: Machine,
    pub subsystem: Subsystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeFormat {
    Pe32,
    Pe32Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    X86,
    X86_64,
    Aarch64,
    Other(u16),
}

impl Machine {
    fn from_coff(value: u16) -> Self {
        match value {
            pe::IMAGE_FILE_MACHINE_I386 => Machine::X86,
            pe::IMAGE_FILE_MACHINE_AMD64 => Machine::X86_64,
            pe::IMAGE_FILE_MACHINE_ARM64 => Machine::Aarch64,
            other => Machine::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Console,
    Gui,
    Other(u16),
}

impl Subsystem {
    fn from_pe(value: u16) -> Self {
        match value {
            pe::IMAGE_SUBSYSTEM_WINDOWS_GUI => Subsystem::Gui,
            pe::IMAGE_SUBSYSTEM_WINDOWS_CUI => Subsystem::Console,
            other => Subsystem::Other(other),
        }
    }
}

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const MZ_MAGIC: &[u8; 2] = b"MZ";
const SHEBANG_MAGIC: &[u8; 2] = b"#!";

pub fn detect(path: &Path) -> Result<BinaryKind, PeError> {
    let mut file = fs::File::open(path).map_err(|e| PeError::io(path, e))?;
    let mut prefix = [0u8; 4];
    let mut filled = 0;
    while filled < prefix.len() {
        let n = file
            .read(&mut prefix[filled..])
            .map_err(|e| PeError::io(path, e))?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    let prefix = &prefix[..filled];
    drop(file);

    if prefix.starts_with(ELF_MAGIC) {
        return Ok(BinaryKind::Elf);
    }
    if prefix.starts_with(SHEBANG_MAGIC) {
        return Ok(BinaryKind::Script);
    }
    if prefix.starts_with(MZ_MAGIC) {
        let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
        return parse_pe_info(path, &data).map(BinaryKind::Pe);
    }
    Ok(BinaryKind::Unknown)
}

pub fn imports(path: &Path) -> Result<Vec<String>, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    match optional_header_magic(&*data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => imports_typed::<ImageNtHeaders32>(path, &data),
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => imports_typed::<ImageNtHeaders64>(path, &data),
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardening {
    pub aslr: bool,
    pub high_entropy_va: bool,
    pub dep: bool,
    pub cfg: bool,
    pub force_integrity: bool,
    pub seh: bool,
    pub signed: bool,
}

pub fn hardening(path: &Path) -> Result<Hardening, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    match optional_header_magic(&*data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => hardening_typed::<ImageNtHeaders32>(path, &data),
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => hardening_typed::<ImageNtHeaders64>(path, &data),
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

fn hardening_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<Hardening, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let dc = file.nt_headers().optional_header().dll_characteristics();
    let has = |flag: u16| dc & flag != 0;
    let signed = file
        .data_directories()
        .get(pe::IMAGE_DIRECTORY_ENTRY_SECURITY)
        .map(|d| d.size.get(LE) != 0 && d.virtual_address.get(LE) != 0)
        .unwrap_or(false);
    Ok(Hardening {
        aslr: has(pe::IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE),
        high_entropy_va: has(pe::IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA),
        dep: has(pe::IMAGE_DLLCHARACTERISTICS_NX_COMPAT),
        cfg: has(pe::IMAGE_DLLCHARACTERISTICS_GUARD_CF),
        force_integrity: has(pe::IMAGE_DLLCHARACTERISTICS_FORCE_INTEGRITY),
        seh: !has(pe::IMAGE_DLLCHARACTERISTICS_NO_SEH),
        signed,
    })
}

fn pe_signature_offset(path: &Path, data: &[u8]) -> Result<usize, PeError> {
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    if data.len() < 0x40 {
        return Err(PeError::malformed(path, "file too small for a DOS header"));
    }
    let e_lfanew = u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as usize;
    if data.len() < e_lfanew + 24 || &data[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return Err(PeError::malformed(path, "missing PE signature at e_lfanew"));
    }
    Ok(e_lfanew)
}

pub fn coff_timestamp(path: &Path) -> Result<u32, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    let off = pe_signature_offset(path, &data)? + 8;
    Ok(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

pub fn set_coff_timestamp(path: &Path, value: u32) -> Result<(), PeError> {
    let mut data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    let off = pe_signature_offset(path, &data)? + 8;
    data[off..off + 4].copy_from_slice(&value.to_le_bytes());
    fs::write(path, &data).map_err(|e| PeError::io(path, e))
}

pub fn exports(path: &Path) -> Result<Vec<String>, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    match optional_header_magic(&*data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => exports_typed::<ImageNtHeaders32>(path, &data),
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => exports_typed::<ImageNtHeaders64>(path, &data),
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

fn exports_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<Vec<String>, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let mut out: Vec<String> = Vec::new();
    let Some(table) = file
        .export_table()
        .map_err(|e| PeError::malformed(path, e))?
    else {
        return Ok(out);
    };
    for export in table.exports().map_err(|e| PeError::malformed(path, e))? {
        match export.name {
            Some(name) => out.push(String::from_utf8_lossy(name).into_owned()),
            None => out.push(format!("#{}", export.ordinal)),
        }
    }
    Ok(out)
}

pub fn imported_symbols(path: &Path) -> Result<Vec<(String, String)>, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    match optional_header_magic(&*data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => {
            imported_symbols_typed::<ImageNtHeaders32>(path, &data)
        }
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => {
            imported_symbols_typed::<ImageNtHeaders64>(path, &data)
        }
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

fn imported_symbols_typed<Pe: ImageNtHeaders>(
    path: &Path,
    data: &[u8],
) -> Result<Vec<(String, String)>, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let mut out: Vec<(String, String)> = Vec::new();
    let Some(table) = file
        .import_table()
        .map_err(|e| PeError::malformed(path, e))?
    else {
        return Ok(out);
    };
    let mut descriptors = table
        .descriptors()
        .map_err(|e| PeError::malformed(path, e))?;
    while let Some(descriptor) = descriptors
        .next()
        .map_err(|e| PeError::malformed(path, e))?
    {
        let dll = String::from_utf8_lossy(
            table
                .name(descriptor.name.get(LE))
                .map_err(|e| PeError::malformed(path, e))?,
        )
        .into_owned();
        let ilt = descriptor.original_first_thunk.get(LE);
        let first = if ilt != 0 {
            ilt
        } else {
            descriptor.first_thunk.get(LE)
        };
        let mut thunks = table
            .thunks(first)
            .map_err(|e| PeError::malformed(path, e))?;
        while let Some(thunk) = thunks
            .next::<Pe>()
            .map_err(|e| PeError::malformed(path, e))?
        {
            let symbol = match table
                .import::<Pe>(thunk)
                .map_err(|e| PeError::malformed(path, e))?
            {
                Import::Ordinal(n) => format!("#{n}"),
                Import::Name(_hint, name) => String::from_utf8_lossy(name).into_owned(),
            };
            out.push((dll.clone(), symbol));
        }
    }
    Ok(out)
}

fn parse_pe_info(path: &Path, data: &[u8]) -> Result<PeInfo, PeError> {
    match optional_header_magic(data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => {
            let file =
                PeFile::<ImageNtHeaders32>::parse(data).map_err(|e| PeError::malformed(path, e))?;
            Ok(pe_info(PeFormat::Pe32, file.nt_headers()))
        }
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => {
            let file =
                PeFile::<ImageNtHeaders64>::parse(data).map_err(|e| PeError::malformed(path, e))?;
            Ok(pe_info(PeFormat::Pe32Plus, file.nt_headers()))
        }
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

fn pe_info<Pe: ImageNtHeaders>(format: PeFormat, nt: &Pe) -> PeInfo {
    PeInfo {
        format,
        machine: Machine::from_coff(nt.file_header().machine.get(LE)),
        subsystem: Subsystem::from_pe(nt.optional_header().subsystem()),
    }
}

fn imports_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<Vec<String>, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let mut dlls: Vec<String> = Vec::new();
    let Some(table) = file
        .import_table()
        .map_err(|e| PeError::malformed(path, e))?
    else {
        return Ok(dlls);
    };
    let mut descriptors = table
        .descriptors()
        .map_err(|e| PeError::malformed(path, e))?;
    while let Some(descriptor) = descriptors
        .next()
        .map_err(|e| PeError::malformed(path, e))?
    {
        let raw = table
            .name(descriptor.name.get(LE))
            .map_err(|e| PeError::malformed(path, e))?;
        let name = String::from_utf8_lossy(raw).into_owned();
        if !dlls.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
            dlls.push(name);
        }
    }
    Ok(dlls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::Command;

    const MINGW_GCC: &str = "x86_64-w64-mingw32-gcc";

    fn write_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    fn build_fixture_exe(dir: &tempfile::TempDir) -> Option<PathBuf> {
        if Command::new(MINGW_GCC).arg("--version").output().is_err() {
            eprintln!("skipping: {MINGW_GCC} not found on PATH");
            return None;
        }
        let src = write_file(
            dir,
            "tick.c",
            b"#include <windows.h>\nint main(void) { return (int)(GetTickCount() & 1); }\n",
        );
        let exe = dir.path().join("tick.exe");
        let out = Command::new(MINGW_GCC)
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .output()
            .expect("failed to spawn mingw gcc");
        assert!(
            out.status.success(),
            "{MINGW_GCC} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Some(exe)
    }

    #[test]
    fn detect_real_pe_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        match detect(&exe).unwrap() {
            BinaryKind::Pe(info) => {
                assert_eq!(info.format, PeFormat::Pe32Plus);
                assert_eq!(info.machine, Machine::X86_64);
                assert_eq!(info.subsystem, Subsystem::Console);
            }
            other => panic!("expected PE, got {other:?}"),
        }
    }

    #[test]
    fn imports_real_pe_fixture_lists_kernel32() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let dlls = imports(&exe).unwrap();
        assert!(
            dlls.iter().any(|d| d.eq_ignore_ascii_case("kernel32.dll")),
            "kernel32.dll not found in {dlls:?}"
        );
        for (i, a) in dlls.iter().enumerate() {
            for b in &dlls[i + 1..] {
                assert!(!a.eq_ignore_ascii_case(b), "duplicate DLL entry {a}");
            }
        }
    }

    #[test]
    fn detect_elf_via_current_exe() {
        let me = std::env::current_exe().unwrap();
        assert_eq!(detect(&me).unwrap(), BinaryKind::Elf);
    }

    #[test]
    fn detect_script_via_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "run.sh", b"#!/bin/sh\necho hi\n");
        assert_eq!(detect(&path).unwrap(), BinaryKind::Script);
    }

    #[test]
    fn detect_unknown_for_random_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "noise.bin", &[0x00, 0xde, 0xad, 0xbe, 0xef, 0x42]);
        assert_eq!(detect(&path).unwrap(), BinaryKind::Unknown);
    }

    #[test]
    fn detect_empty_file_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "empty", b"");
        assert_eq!(detect(&path).unwrap(), BinaryKind::Unknown);
    }

    #[test]
    fn detect_truncated_mz_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "trunc.exe", b"MZ");
        let err = detect(&path).unwrap_err();
        assert!(matches!(err, PeError::MalformedPe { .. }), "got {err:?}");
        assert!(err.to_string().starts_with("LSW1302"));
    }

    #[test]
    fn detect_mz_with_garbage_headers_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = vec![0u8; 128];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        let path = write_file(&dir, "garbage.exe", &bytes);
        assert!(matches!(
            detect(&path).unwrap_err(),
            PeError::MalformedPe { .. }
        ));
    }

    #[test]
    fn detect_missing_file_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = detect(&dir.path().join("absent.exe")).unwrap_err();
        assert!(matches!(err, PeError::Io { .. }), "got {err:?}");
        assert!(err.to_string().starts_with("LSW1301"));
    }

    #[test]
    fn coff_timestamp_can_be_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        set_coff_timestamp(&exe, 0).unwrap();
        assert_eq!(coff_timestamp(&exe).unwrap(), 0);
        set_coff_timestamp(&exe, 0).unwrap();
        assert_eq!(coff_timestamp(&exe).unwrap(), 0);
    }

    #[test]
    fn hardening_reads_dll_characteristics_of_a_real_pe() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let h = hardening(&exe).unwrap();
        assert!(h.aslr, "mingw enables DYNAMICBASE by default");
        assert!(h.dep, "mingw enables NXCOMPAT by default");
        assert!(!h.signed, "a freshly built exe is unsigned");
    }

    #[test]
    fn hardening_rejects_non_pe() {
        let dir = tempfile::tempdir().unwrap();
        let me = std::env::current_exe().unwrap_or_else(|_| dir.path().join("x"));
        let _ = hardening(&me);
        let txt = write_file(&dir, "n.txt", b"not a pe");
        assert!(matches!(
            hardening(&txt).unwrap_err(),
            PeError::NotPe { .. }
        ));
    }

    #[test]
    fn imported_symbols_lists_named_functions() {
        let dir = tempfile::tempdir().unwrap();
        let Some(exe) = build_fixture_exe(&dir) else {
            return;
        };
        let symbols = imported_symbols(&exe).unwrap();
        assert!(!symbols.is_empty(), "expected named imports");
        assert!(
            symbols
                .iter()
                .any(|(dll, func)| dll.eq_ignore_ascii_case("KERNEL32.dll")
                    && func == "GetTickCount"),
            "GetTickCount import not found in {symbols:?}"
        );
    }

    #[test]
    fn imports_rejects_non_pe() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "script.sh", b"#!/bin/sh\n");
        let err = imports(&path).unwrap_err();
        assert!(matches!(err, PeError::NotPe { .. }), "got {err:?}");
        assert!(err.to_string().starts_with("LSW1303"));
    }

    #[test]
    fn imports_rejects_elf() {
        let me = std::env::current_exe().unwrap();
        assert!(matches!(imports(&me).unwrap_err(), PeError::NotPe { .. }));
    }

    #[test]
    fn imports_truncated_mz_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(&dir, "trunc.exe", b"MZ");
        assert!(matches!(
            imports(&path).unwrap_err(),
            PeError::MalformedPe { .. }
        ));
    }

    #[test]
    fn imports_missing_file_is_io_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            imports(&dir.path().join("absent.exe")).unwrap_err(),
            PeError::Io { .. }
        ));
    }

    #[test]
    fn machine_and_subsystem_mappings() {
        assert_eq!(Machine::from_coff(0x014c), Machine::X86);
        assert_eq!(Machine::from_coff(0x8664), Machine::X86_64);
        assert_eq!(Machine::from_coff(0xaa64), Machine::Aarch64);
        assert_eq!(Machine::from_coff(0x01c4), Machine::Other(0x01c4));
        assert_eq!(Subsystem::from_pe(2), Subsystem::Gui);
        assert_eq!(Subsystem::from_pe(3), Subsystem::Console);
        assert_eq!(Subsystem::from_pe(1), Subsystem::Other(1));
    }

    #[test]
    fn error_messages_carry_stable_ids_and_paths() {
        let io = PeError::io(
            Path::new("/x/y.exe"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        assert!(io.to_string().contains("LSW1301"));
        assert!(io.to_string().contains("/x/y.exe"));

        let mal = PeError::malformed(Path::new("/x/y.exe"), "bad header");
        assert!(mal.to_string().contains("LSW1302"));
        assert!(mal.to_string().contains("bad header"));

        let not_pe = PeError::NotPe {
            path: PathBuf::from("/x/y.sh"),
        };
        assert!(not_pe.to_string().contains("LSW1303"));
        assert!(not_pe.to_string().contains("/x/y.sh"));
    }
}
