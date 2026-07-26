use std::io::Read;
use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile};

use crate::MZ_MAGIC;
use crate::error::PeError;
use crate::image::{PeImage, dispatch_pe};
use crate::types::*;

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const SHEBANG_MAGIC: &[u8; 2] = b"#!";
const DETECT_HEADER_BYTES: usize = 64 * 1024;

pub fn detect(path: &Path) -> Result<BinaryKind, PeError> {
    let prefix = read_prefix(path, DETECT_HEADER_BYTES)?;

    if prefix.starts_with(ELF_MAGIC) {
        return Ok(BinaryKind::Elf);
    }
    if prefix.starts_with(SHEBANG_MAGIC) {
        return Ok(BinaryKind::Script);
    }
    if prefix.starts_with(MZ_MAGIC) {
        if let Ok(meta) = std::fs::metadata(path)
            && meta.len() > crate::error::MAX_PE_BYTES
        {
            return Err(PeError::malformed(
                path,
                format!(
                    "file exceeds {}-byte limit for PE parsing",
                    crate::error::MAX_PE_BYTES
                ),
            ));
        }
        return match parse_pe_info(path, &prefix) {
            Ok(info) => Ok(BinaryKind::Pe(info)),
            Err(_) if prefix.len() == DETECT_HEADER_BYTES => {
                let data = crate::error::read_pe(path)?;
                parse_pe_info(path, &data).map(BinaryKind::Pe)
            }
            Err(e) => Err(e),
        };
    }
    Ok(BinaryKind::Unknown)
}

impl PeImage {
    pub fn info(&self) -> Result<PeInfo, PeError> {
        parse_pe_info(&self.path, &self.data)
    }
}

fn read_prefix(path: &Path, max: usize) -> Result<Vec<u8>, PeError> {
    let file = std::fs::File::open(path).map_err(|e| PeError::io(path, e))?;
    let mut data = Vec::new();
    file.take(max as u64)
        .read_to_end(&mut data)
        .map_err(|e| PeError::io(path, e))?;
    Ok(data)
}

pub(crate) fn parse_pe_info(path: &Path, data: &[u8]) -> Result<PeInfo, PeError> {
    dispatch_pe!(path, data, pe_info_typed)
}

fn pe_info_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<PeInfo, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let nt = file.nt_headers();
    let format = if nt.optional_header().magic() == pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC {
        PeFormat::Pe32Plus
    } else {
        PeFormat::Pe32
    };
    Ok(PeInfo {
        format,
        machine: Machine::from_coff(nt.file_header().machine.get(LE)),
        subsystem: Subsystem::from_pe(nt.optional_header().subsystem()),
    })
}
