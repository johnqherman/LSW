use std::path::Path;

use object::LittleEndian as LE;
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile};

use crate::error::PeError;
use crate::image::{PeImage, dispatch_pe};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    pub name: String,
    pub virtual_size: u32,
    pub raw_size: u32,
    pub raw_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeDetails {
    pub entry_point: u32,
    pub image_base: u64,
    pub sections: Vec<SectionInfo>,
}

pub fn details(path: &Path) -> Result<PeDetails, PeError> {
    PeImage::open(path)?.details()
}

impl PeImage {
    pub fn details(&self) -> Result<PeDetails, PeError> {
        dispatch_pe!(&self.path, &self.data, details_typed)
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
            raw_offset: section.pointer_to_raw_data.get(LE),
        });
    }
    Ok(PeDetails {
        entry_point: oh.address_of_entry_point(),
        image_base: oh.image_base(),
        sections,
    })
}
