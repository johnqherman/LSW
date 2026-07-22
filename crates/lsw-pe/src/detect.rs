use std::fs;
use std::io::Read;
use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile, optional_header_magic};

use crate::MZ_MAGIC;
use crate::error::PeError;
use crate::types::*;

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
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
