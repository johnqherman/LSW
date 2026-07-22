use std::fs;
use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, PeFile, optional_header_magic};

use crate::MZ_MAGIC;
use crate::error::PeError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resources {
    pub manifest: Option<String>,
    pub execution_level: Option<String>,
    pub dpi_aware: Option<String>,
    pub version: std::collections::BTreeMap<String, String>,
    pub has_icon: bool,
}

const RT_ICON_GROUP: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;

pub fn resources(path: &Path) -> Result<Resources, PeError> {
    let data = fs::read(path).map_err(|e| PeError::io(path, e))?;
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    match optional_header_magic(&*data).map_err(|e| PeError::malformed(path, e))? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => resources_typed::<ImageNtHeaders32>(path, &data),
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => resources_typed::<ImageNtHeaders64>(path, &data),
        other => Err(PeError::malformed(
            path,
            format!("unrecognized optional header magic 0x{other:04x}"),
        )),
    }
}

fn rva_to_bytes<'d, Pe: ImageNtHeaders>(
    file: &PeFile<'d, Pe>,
    data: &'d [u8],
    rva: u32,
    size: u32,
) -> Option<&'d [u8]> {
    for section in file.section_table().iter() {
        let va = section.virtual_address.get(LE);
        let vsize = section.virtual_size.get(LE);
        let raw = section.size_of_raw_data.get(LE);
        let span = vsize.max(raw);
        if rva >= va && rva < va.saturating_add(span) {
            let ptr = section.pointer_to_raw_data.get(LE);
            let start = ptr.checked_add(rva - va)? as usize;
            let end = start.checked_add(size as usize)?;
            return data.get(start..end.min(data.len()));
        }
    }
    None
}

fn resources_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<Resources, PeError> {
    use object::read::pe::ResourceDirectoryEntryData::{Data, Table};
    use object::read::pe::ResourceNameOrId;

    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let mut out = Resources::default();
    let sections = file.section_table();
    let Some(dir) = file
        .data_directories()
        .resource_directory(data, &sections)
        .map_err(|e| PeError::malformed(path, e))?
    else {
        return Ok(out);
    };
    let root = dir.root().map_err(|e| PeError::malformed(path, e))?;

    for type_entry in root.entries {
        let id = match type_entry.name_or_id() {
            ResourceNameOrId::Id(id) => id,
            ResourceNameOrId::Name(_) => continue,
        };
        if id != RT_ICON_GROUP && id != RT_VERSION && id != RT_MANIFEST {
            continue;
        }
        if id == RT_ICON_GROUP {
            out.has_icon = true;
            continue;
        }
        let Ok(Table(names)) = type_entry.data(dir) else {
            continue;
        };
        for name_entry in names.entries {
            let Ok(Table(langs)) = name_entry.data(dir) else {
                continue;
            };
            for lang_entry in langs.entries {
                let Ok(Data(entry)) = lang_entry.data(dir) else {
                    continue;
                };
                let Some(bytes) = rva_to_bytes(
                    &file,
                    data,
                    entry.offset_to_data.get(LE),
                    entry.size.get(LE),
                ) else {
                    continue;
                };
                match id {
                    RT_MANIFEST => parse_manifest(bytes, &mut out),
                    RT_VERSION => parse_version(bytes, &mut out.version),
                    _ => {}
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn parse_manifest(bytes: &[u8], out: &mut Resources) {
    let text = String::from_utf8_lossy(bytes).into_owned();
    out.execution_level = between(&text, "level=\"", "\"");
    if let Some(dpi) = between(&text, "<dpiAware>", "</dpiAware>") {
        out.dpi_aware = Some(dpi);
    } else if text.contains("dpiAwareness") {
        out.dpi_aware = between(&text, "<dpiAwareness>", "</dpiAwareness>");
    }
    out.manifest = Some(text);
}

fn between(text: &str, start: &str, end: &str) -> Option<String> {
    let s = text.find(start)? + start.len();
    let rest = &text[s..];
    let e = rest.find(end)?;
    Some(rest[..e].trim().to_owned())
}

pub(crate) fn parse_version(bytes: &[u8], out: &mut std::collections::BTreeMap<String, String>) {
    let wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let tokens: Vec<String> = wide
        .split(|&u| u == 0)
        .filter(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    const KEYS: &[&str] = &[
        "FileVersion",
        "ProductVersion",
        "ProductName",
        "CompanyName",
        "FileDescription",
        "OriginalFilename",
        "LegalCopyright",
    ];
    let mut i = 0;
    while i + 1 < tokens.len() {
        if KEYS.contains(&tokens[i].as_str()) {
            out.insert(tokens[i].clone(), tokens[i + 1].clone());
            i += 2;
        } else {
            i += 1;
        }
    }
}
