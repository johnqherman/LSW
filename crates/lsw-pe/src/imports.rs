use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, Import, PeFile, optional_header_magic};

use crate::MZ_MAGIC;
use crate::error::PeError;

const MAX_NAMES: usize = 65536;
const MAX_NAME_LEN: usize = 512;
const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;

fn decode_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(&raw[..raw.len().min(MAX_NAME_LEN)]).into_owned()
}

pub fn imports(path: &Path) -> Result<Vec<String>, PeError> {
    let data = crate::error::read_pe(path)?;
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
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut visited = 0usize;
    let mut scanned = 0usize;
    while let Some(descriptor) = descriptors
        .next()
        .map_err(|e| PeError::malformed(path, e))?
    {
        if dlls.len() >= MAX_NAMES || visited >= MAX_NAMES || scanned >= MAX_SCAN_BYTES {
            break;
        }
        visited += 1;
        let raw = table
            .name(descriptor.name.get(LE))
            .map_err(|e| PeError::malformed(path, e))?;
        scanned += raw.len();
        let name = decode_name(raw);
        if seen.insert(name.to_ascii_lowercase()) {
            dlls.push(name);
        }
    }
    Ok(dlls)
}

pub fn exports(path: &Path) -> Result<Vec<String>, PeError> {
    let data = crate::error::read_pe(path)?;
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
    let ordinal_base = table.ordinal_base();
    let count = table.addresses().len().min(MAX_NAMES);
    let mut names: std::collections::HashMap<u32, &[u8]> = std::collections::HashMap::new();
    let mut scanned = 0usize;
    for (name_pointer, ordinal_index) in table.name_iter() {
        if names.len() >= MAX_NAMES || scanned >= MAX_SCAN_BYTES {
            break;
        }
        if let Ok(name) = table.name_from_pointer(name_pointer) {
            scanned += name.len();
            names.entry(ordinal_index as u32).or_insert(name);
        }
    }
    for i in 0..count {
        match names.get(&(i as u32)) {
            Some(name) => out.push(decode_name(name)),
            None => out.push(format!("#{}", ordinal_base.wrapping_add(i as u32))),
        }
    }
    Ok(out)
}

pub fn imported_symbols(path: &Path) -> Result<Vec<(String, String)>, PeError> {
    let data = crate::error::read_pe(path)?;
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
    let mut visited = 0usize;
    let mut scanned = 0usize;
    while let Some(descriptor) = descriptors
        .next()
        .map_err(|e| PeError::malformed(path, e))?
    {
        if out.len() >= MAX_NAMES || visited >= MAX_NAMES || scanned >= MAX_SCAN_BYTES {
            break;
        }
        visited += 1;
        let dll_raw = table
            .name(descriptor.name.get(LE))
            .map_err(|e| PeError::malformed(path, e))?;
        scanned += dll_raw.len();
        let dll = decode_name(dll_raw);
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
            if out.len() >= MAX_NAMES || scanned >= MAX_SCAN_BYTES {
                break;
            }
            let symbol = match table
                .import::<Pe>(thunk)
                .map_err(|e| PeError::malformed(path, e))?
            {
                Import::Ordinal(n) => format!("#{n}"),
                Import::Name(_hint, name) => {
                    scanned += name.len();
                    decode_name(name)
                }
            };
            out.push((dll.clone(), symbol));
        }
    }
    Ok(out)
}
