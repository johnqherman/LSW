use std::fs;
use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile, optional_header_magic};

use crate::MZ_MAGIC;
use crate::error::PeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    pub name: String,
    pub virtual_size: u32,
    pub raw_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeDetails {
    pub entry_point: u32,
    pub image_base: u64,
    pub sections: Vec<SectionInfo>,
}

pub fn details(path: &Path) -> Result<PeDetails, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    match optional_header_magic(&*data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => details_typed::<ImageNtHeaders32>(path, &data),
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => details_typed::<ImageNtHeaders64>(path, &data),
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

fn details_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<PeDetails, PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let oh = file.nt_headers().optional_header();
    let mut sections = Vec::new();
    for section in file.section_table().iter() {
        sections.push(SectionInfo {
            name: String::from_utf8_lossy(section.name.as_slice())
                .trim_end_matches('\0')
                .to_owned(),
            virtual_size: section.virtual_size.get(LE),
            raw_size: section.size_of_raw_data.get(LE),
        });
    }
    Ok(PeDetails {
        entry_point: oh.address_of_entry_point(),
        image_base: oh.image_base(),
        sections,
    })
}
