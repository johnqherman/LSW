use std::fs;
use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile, optional_header_magic};

use crate::MZ_MAGIC;
use crate::error::PeError;

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
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => {
            hardening_typed::<ImageNtHeaders32>(path, &data, false)
        }
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => hardening_typed::<ImageNtHeaders64>(path, &data, true),
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

struct LoadCfg {
    size: usize,
    se_table: u64,
    se_count: u64,
    guard_flags: u32,
}

fn load_config<Pe: ImageNtHeaders>(file: &PeFile<Pe>, data: &[u8], is_64: bool) -> Option<LoadCfg> {
    let sections = file.section_table();
    let dir = file
        .data_directories()
        .get(pe::IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG)?;
    let bytes = dir.data(data, &sections).ok()?;
    if is_64 {
        let (cfg, _) = object::pod::from_bytes::<pe::ImageLoadConfigDirectory64>(bytes).ok()?;
        Some(LoadCfg {
            size: cfg.size.get(LE) as usize,
            se_table: cfg.sehandler_table.get(LE),
            se_count: cfg.sehandler_count.get(LE),
            guard_flags: cfg.guard_flags.get(LE),
        })
    } else {
        let (cfg, _) = object::pod::from_bytes::<pe::ImageLoadConfigDirectory32>(bytes).ok()?;
        Some(LoadCfg {
            size: cfg.size.get(LE) as usize,
            se_table: cfg.sehandler_table.get(LE) as u64,
            se_count: cfg.sehandler_count.get(LE) as u64,
            guard_flags: cfg.guard_flags.get(LE),
        })
    }
}

fn hardening_typed<Pe: ImageNtHeaders>(
    path: &Path,
    data: &[u8],
    is_64: bool,
) -> Result<Hardening, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let dc = file.nt_headers().optional_header().dll_characteristics();
    let file_chars = file.nt_headers().file_header().characteristics.get(LE);
    let has = |flag: u16| dc & flag != 0;
    let relocs_stripped = file_chars & pe::IMAGE_FILE_RELOCS_STRIPPED != 0;
    let lc = load_config(&file, data, is_64);
    let se_off = std::mem::offset_of!(pe::ImageLoadConfigDirectory32, sehandler_count) + 4;
    let seh = if is_64 {
        true
    } else {
        has(pe::IMAGE_DLLCHARACTERISTICS_NO_SEH)
            || lc
                .as_ref()
                .is_some_and(|c| c.size >= se_off && c.se_table != 0 && c.se_count > 0)
    };
    let gf_off = if is_64 {
        std::mem::offset_of!(pe::ImageLoadConfigDirectory64, guard_flags) + 4
    } else {
        std::mem::offset_of!(pe::ImageLoadConfigDirectory32, guard_flags) + 4
    };
    let cfg = has(pe::IMAGE_DLLCHARACTERISTICS_GUARD_CF)
        && lc.as_ref().is_some_and(|c| {
            c.size >= gf_off && c.guard_flags & pe::IMAGE_GUARD_CF_INSTRUMENTED != 0
        });
    let signed = file
        .data_directories()
        .get(pe::IMAGE_DIRECTORY_ENTRY_SECURITY)
        .map(|d| {
            let addr = d.virtual_address.get(LE) as usize;
            let size = d.size.get(LE) as usize;
            if addr == 0 || size < 8 {
                return false;
            }
            let Some(hdr) = data.get(addr..addr.saturating_add(8)) else {
                return false;
            };
            let dw_length = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
            let revision = u16::from_le_bytes([hdr[4], hdr[5]]);
            let cert_type = u16::from_le_bytes([hdr[6], hdr[7]]);
            dw_length >= 8
                && dw_length <= size
                && addr.saturating_add(dw_length) <= data.len()
                && matches!(revision, 0x0100 | 0x0200)
                && cert_type == 0x0002
        })
        .unwrap_or(false);
    Ok(Hardening {
        aslr: has(pe::IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE) && !relocs_stripped,
        high_entropy_va: is_64 && has(pe::IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA),
        dep: has(pe::IMAGE_DLLCHARACTERISTICS_NX_COMPAT),
        cfg,
        force_integrity: has(pe::IMAGE_DLLCHARACTERISTICS_FORCE_INTEGRITY),
        seh,
        signed,
    })
}
