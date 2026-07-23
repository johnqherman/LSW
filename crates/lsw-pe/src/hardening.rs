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

fn safe_seh<Pe: ImageNtHeaders>(file: &PeFile<Pe>, data: &[u8], is_64: bool, no_seh: bool) -> bool {
    if is_64 {
        return true;
    }
    if no_seh {
        return true;
    }
    let sections = file.section_table();
    let Some(dir) = file
        .data_directories()
        .get(pe::IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG)
    else {
        return false;
    };
    let Ok(bytes) = dir.data(data, &sections) else {
        return false;
    };
    match object::pod::from_bytes::<pe::ImageLoadConfigDirectory32>(bytes) {
        Ok((cfg, _)) => {
            let size = cfg.size.get(LE) as usize;
            let need = std::mem::offset_of!(pe::ImageLoadConfigDirectory32, sehandler_count) + 4;
            size >= need && cfg.sehandler_count.get(LE) > 0
        }
        Err(_) => false,
    }
}

fn hardening_typed<Pe: ImageNtHeaders>(
    path: &Path,
    data: &[u8],
    is_64: bool,
) -> Result<Hardening, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let dc = file.nt_headers().optional_header().dll_characteristics();
    let has = |flag: u16| dc & flag != 0;
    let seh = safe_seh(&file, data, is_64, has(pe::IMAGE_DLLCHARACTERISTICS_NO_SEH));
    let signed = file
        .data_directories()
        .get(pe::IMAGE_DIRECTORY_ENTRY_SECURITY)
        .map(|d| {
            let addr = d.virtual_address.get(LE) as u64;
            let size = d.size.get(LE) as u64;
            addr != 0 && size != 0 && addr.saturating_add(size) <= data.len() as u64
        })
        .unwrap_or(false);
    Ok(Hardening {
        aslr: has(pe::IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE),
        high_entropy_va: has(pe::IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA),
        dep: has(pe::IMAGE_DLLCHARACTERISTICS_NX_COMPAT),
        cfg: has(pe::IMAGE_DLLCHARACTERISTICS_GUARD_CF),
        force_integrity: has(pe::IMAGE_DLLCHARACTERISTICS_FORCE_INTEGRITY),
        seh,
        signed,
    })
}
