use std::path::Path;

use object::LittleEndian as LE;
use object::read::pe::{ImageNtHeaders, PeFile};

use crate::error::PeError;
use crate::image::{PeImage, dispatch_pe};

/// Extracted PE resource data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resources {
    /// Application manifest XML, if present.
    pub manifest: Option<String>,
    /// Requested execution level from the manifest.
    pub execution_level: Option<String>,
    /// DPI awareness setting from the manifest.
    pub dpi_aware: Option<String>,
    /// Version info string table entries.
    pub version: std::collections::BTreeMap<String, String>,
    /// Whether the image contains an icon resource.
    pub has_icon: bool,
}

const RT_ICON_GROUP: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;
const MAX_RESOURCE_VISITS: u32 = 100_000;
const MAX_RESOURCE_DATA: usize = 4 * 1024 * 1024;
const MAX_RESOURCE_PAYLOAD_TOTAL: usize = 64 * 1024 * 1024;

/// Extracts resources from a PE file on disk.
pub fn resources(path: &Path) -> Result<Resources, PeError> {
    PeImage::open(path)?.resources()
}

impl PeImage {
    /// Extracts resources from the loaded image.
    pub fn resources(&self) -> Result<Resources, PeError> {
        dispatch_pe!(&self.path, &self.data, resources_typed)
    }
}

fn rva_to_bytes<'d, Pe: ImageNtHeaders>(
    file: &PeFile<'d, Pe>,
    data: &'d [u8],
    rva: u32,
    size: u32,
) -> Option<&'d [u8]> {
    let bytes = file.section_table().pe_data_at(data, rva)?;
    Some(&bytes[..bytes.len().min(size as usize)])
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

    let mut budget: u32 = MAX_RESOURCE_VISITS;
    let mut payload_budget: usize = MAX_RESOURCE_PAYLOAD_TOTAL;
    'types: for type_entry in root.entries {
        if budget == 0 {
            break 'types;
        }
        budget -= 1;
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
            if budget == 0 {
                break 'types;
            }
            budget -= 1;
            let Ok(Table(langs)) = name_entry.data(dir) else {
                continue;
            };
            for lang_entry in langs.entries {
                if budget == 0 {
                    break 'types;
                }
                budget -= 1;
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
                let bytes = &bytes[..bytes.len().min(MAX_RESOURCE_DATA)];
                if payload_budget < bytes.len() {
                    break 'types;
                }
                payload_budget -= bytes.len();
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

fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let wide: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&wide)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let wide: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&wide)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

pub(crate) fn parse_manifest(bytes: &[u8], out: &mut Resources) {
    let text = decode_text(bytes);
    out.execution_level = between(&text, "level=\"", "\"");
    if let Some(dpi) = element_content(&text, "dpiAware") {
        out.dpi_aware = Some(dpi);
    } else if let Some(dpi) = element_content(&text, "dpiAwareness") {
        out.dpi_aware = Some(dpi);
    }
    out.manifest = Some(text);
}

fn between(text: &str, start: &str, end: &str) -> Option<String> {
    let s = text.find(start)? + start.len();
    let rest = &text[s..];
    let e = rest.find(end)?;
    Some(rest[..e].trim().to_owned())
}

fn element_content(text: &str, local_name: &str) -> Option<String> {
    let open_tag = format!("<{local_name}");
    let close_tag = format!("</{local_name}>");
    let start = text.find(&open_tag)?;
    let after_tag = &text[start + open_tag.len()..];
    let gt = after_tag.find('>')?;
    let content_start = &after_tag[gt + 1..];
    let end = content_start.find(&close_tag)?;
    Some(content_start[..end].trim().to_owned())
}

pub(crate) fn parse_version(bytes: &[u8], out: &mut std::collections::BTreeMap<String, String>) {
    const KEYS: &[&str] = &[
        "FileVersion",
        "ProductVersion",
        "ProductName",
        "CompanyName",
        "FileDescription",
        "InternalName",
        "OriginalFilename",
        "LegalCopyright",
    ];
    let Some(children_start) = skip_version_node(bytes, "VS_VERSION_INFO") else {
        return;
    };
    let Some(string_fi_start) = find_child_node(&bytes[children_start..], "StringFileInfo") else {
        return;
    };
    let abs = children_start + string_fi_start;
    let Some(table_start) = skip_version_node(&bytes[abs..], "StringFileInfo") else {
        return;
    };
    parse_string_table(&bytes[abs + table_start..], KEYS, out);
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let b = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn read_wstring(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let mut end = offset;
    loop {
        let word = read_u16_le(bytes, end)?;
        end += 2;
        if word == 0 {
            break;
        }
    }
    let words: Vec<u16> = bytes[offset..end - 2]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some((String::from_utf16_lossy(&words), end))
}

fn align4(offset: usize) -> usize {
    (offset + 3) & !3
}

fn skip_version_node(bytes: &[u8], expected_key: &str) -> Option<usize> {
    if bytes.len() < 6 {
        return None;
    }
    let _w_length = read_u16_le(bytes, 0)?;
    let w_value_length = read_u16_le(bytes, 2)?;
    let _w_type = read_u16_le(bytes, 4)?;
    let (key, after_key) = read_wstring(bytes, 6)?;
    if key != expected_key {
        return None;
    }
    let value_start = align4(after_key);
    let value_bytes = w_value_length as usize;
    let children_start = align4(value_start + value_bytes);
    Some(children_start)
}

fn find_child_node(bytes: &[u8], key: &str) -> Option<usize> {
    let mut offset = 0;
    let mut budget = 256u32;
    while offset + 6 <= bytes.len() && budget > 0 {
        budget -= 1;
        let w_length = read_u16_le(bytes, offset)? as usize;
        if w_length < 6 || offset + w_length > bytes.len() {
            break;
        }
        let (found_key, _) = read_wstring(bytes, offset + 6)?;
        if found_key == key {
            return Some(offset);
        }
        offset = align4(offset + w_length);
    }
    None
}

fn parse_string_table(
    bytes: &[u8],
    keys: &[&str],
    out: &mut std::collections::BTreeMap<String, String>,
) {
    let Some(entries_start) = skip_version_node(bytes, "").or_else(|| {
        if bytes.len() < 6 {
            return None;
        }
        let (_, after_key) = read_wstring(bytes, 6)?;
        Some(align4(after_key))
    }) else {
        return;
    };
    let table_len = read_u16_le(bytes, 0).unwrap_or(0) as usize;
    let table_end = table_len.min(bytes.len());
    let mut offset = entries_start;
    let mut budget = 1024u32;
    while offset + 6 <= table_end && budget > 0 {
        budget -= 1;
        let w_length = read_u16_le(bytes, offset).unwrap_or(0) as usize;
        if w_length < 6 {
            break;
        }
        let entry_end = (offset + w_length).min(table_end);
        let w_value_length = read_u16_le(bytes, offset + 2).unwrap_or(0) as usize;
        if let Some((key, after_key)) = read_wstring(bytes, offset + 6)
            && keys.contains(&key.as_str())
            && w_value_length > 0
        {
            let value_start = align4(after_key);
            let value_chars = w_value_length.saturating_sub(1);
            let value_end = (value_start + value_chars * 2).min(entry_end);
            if value_start < value_end {
                let words: Vec<u16> = bytes[value_start..value_end]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                out.insert(key, String::from_utf16_lossy(&words));
            }
        }
        offset = align4(entry_end);
    }
}
