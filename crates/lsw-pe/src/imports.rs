use std::fs;
use std::path::Path;

use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, Import, PeFile, optional_header_magic};

use crate::MZ_MAGIC;
use crate::error::PeError;

const MAX_NAMES: usize = 65536;
const MAX_NAME_LEN: usize = 4096;

fn decode_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(&raw[..raw.len().min(MAX_NAME_LEN)]).into_owned()
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
    let mut visited = 0usize;
    while let Some(descriptor) = descriptors
        .next()
        .map_err(|e| PeError::malformed(path, e))?
    {
        if dlls.len() >= MAX_NAMES || visited >= MAX_NAMES {
            break;
        }
        visited += 1;
        let raw = table
            .name(descriptor.name.get(LE))
            .map_err(|e| PeError::malformed(path, e))?;
        let name = decode_name(raw);
        if !dlls.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
            dlls.push(name);
        }
    }
    Ok(dlls)
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
        if out.len() >= MAX_NAMES {
            break;
        }
        match export.name {
            Some(name) => out.push(decode_name(name)),
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
    let mut visited = 0usize;
    while let Some(descriptor) = descriptors
        .next()
        .map_err(|e| PeError::malformed(path, e))?
    {
        if out.len() >= MAX_NAMES || visited >= MAX_NAMES {
            break;
        }
        visited += 1;
        let dll = decode_name(
            table
                .name(descriptor.name.get(LE))
                .map_err(|e| PeError::malformed(path, e))?,
        );
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
            if out.len() >= MAX_NAMES {
                break;
            }
            let symbol = match table
                .import::<Pe>(thunk)
                .map_err(|e| PeError::malformed(path, e))?
            {
                Import::Ordinal(n) => format!("#{n}"),
                Import::Name(_hint, name) => decode_name(name),
            };
            out.push((dll.clone(), symbol));
        }
    }
    Ok(out)
}
