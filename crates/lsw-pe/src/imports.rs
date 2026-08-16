use std::path::Path;

use object::LittleEndian as LE;
use object::read::pe::{ImageNtHeaders, Import, PeFile};

use crate::error::PeError;
use crate::image::{PeImage, dispatch_pe};

const MAX_NAMES: usize = 65536;
const MAX_NAME_LEN: usize = 512;
const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;

fn decode_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(&raw[..raw.len().min(MAX_NAME_LEN)]).into_owned()
}

/// Returns the list of DLL names imported by the PE at the given path.
pub fn imports(path: &Path) -> Result<Vec<String>, PeError> {
    PeImage::open(path)?.imports()
}

impl PeImage {
    /// Returns imported DLL names.
    pub fn imports(&self) -> Result<Vec<String>, PeError> {
        dispatch_pe!(&self.path, &self.data, imports_typed)
    }

    /// Returns exported symbol names.
    pub fn exports(&self) -> Result<Vec<String>, PeError> {
        dispatch_pe!(&self.path, &self.data, exports_typed)
    }

    /// Returns (DLL, symbol) pairs for all imported symbols.
    pub fn imported_symbols(&self) -> Result<Vec<(String, String)>, PeError> {
        dispatch_pe!(&self.path, &self.data, imported_symbols_typed)
    }
}

fn walk_import_descriptors<Pe: ImageNtHeaders>(
    path: &Path,
    data: &[u8],
    mut visit: impl FnMut(
        &object::read::pe::ImportTable,
        &object::pe::ImageImportDescriptor,
        String,
        &mut usize,
    ) -> Result<bool, PeError>,
) -> Result<(), PeError> {
    let file = PeFile::<Pe>::parse(data).map_err(|e| PeError::malformed(path, e))?;
    let Some(table) = file
        .import_table()
        .map_err(|e| PeError::malformed(path, e))?
    else {
        return Ok(());
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
        if visited >= MAX_NAMES || scanned >= MAX_SCAN_BYTES {
            break;
        }
        visited += 1;
        let raw = table
            .name(descriptor.name.get(LE))
            .map_err(|e| PeError::malformed(path, e))?;
        scanned += raw.len();
        let name = decode_name(raw);
        if !visit(&table, descriptor, name, &mut scanned)? {
            break;
        }
    }
    Ok(())
}

fn imports_typed<Pe: ImageNtHeaders>(path: &Path, data: &[u8]) -> Result<Vec<String>, PeError> {
    let mut dlls: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    walk_import_descriptors::<Pe>(path, data, |_, _, name, _| {
        if seen.insert(name.to_ascii_lowercase()) {
            dlls.push(name);
        }
        Ok(dlls.len() < MAX_NAMES)
    })?;
    Ok(dlls)
}

/// Returns the list of exported symbol names from the PE at the given path.
pub fn exports(path: &Path) -> Result<Vec<String>, PeError> {
    PeImage::open(path)?.exports()
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
            names.entry(u32::from(ordinal_index)).or_insert(name);
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

/// Returns (DLL, symbol) pairs for all imported symbols from a file on disk.
pub fn imported_symbols(path: &Path) -> Result<Vec<(String, String)>, PeError> {
    PeImage::open(path)?.imported_symbols()
}

fn imported_symbols_typed<Pe: ImageNtHeaders>(
    path: &Path,
    data: &[u8],
) -> Result<Vec<(String, String)>, PeError> {
    let mut out: Vec<(String, String)> = Vec::new();
    walk_import_descriptors::<Pe>(path, data, |table, descriptor, dll, scanned| {
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
            if out.len() >= MAX_NAMES || *scanned >= MAX_SCAN_BYTES {
                break;
            }
            let symbol = match table
                .import::<Pe>(thunk)
                .map_err(|e| PeError::malformed(path, e))?
            {
                Import::Ordinal(n) => format!("#{n}"),
                Import::Name(_hint, name) => {
                    *scanned += name.len();
                    decode_name(name)
                }
            };
            out.push((dll.clone(), symbol));
        }
        Ok(out.len() < MAX_NAMES)
    })?;
    Ok(out)
}
