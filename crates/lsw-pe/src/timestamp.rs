use std::fs;
use std::path::Path;

use crate::MZ_MAGIC;
use crate::error::PeError;

fn pe_signature_offset(path: &Path, data: &[u8]) -> Result<usize, PeError> {
    if !data.starts_with(MZ_MAGIC) {
        return Err(PeError::NotPe {
            path: path.to_path_buf(),
        });
    }
    if data.len() < 0x40 {
        return Err(PeError::malformed(path, "file too small for a DOS header"));
    }
    let e_lfanew = u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as usize;
    if e_lfanew.checked_add(24).is_none_or(|end| data.len() < end)
        || &data[e_lfanew..e_lfanew + 4] != b"PE\0\0"
    {
        return Err(PeError::malformed(path, "missing PE signature at e_lfanew"));
    }
    Ok(e_lfanew)
}

pub fn coff_timestamp(path: &Path) -> Result<u32, PeError> {
    let data = crate::error::read_pe(path)?;
    let off = pe_signature_offset(path, &data)? + 8;
    Ok(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

pub fn set_coff_timestamp(path: &Path, value: u32) -> Result<(), PeError> {
    let mut data = crate::error::read_pe(path)?;
    let off = pe_signature_offset(path, &data)? + 8;
    data[off..off + 4].copy_from_slice(&value.to_le_bytes());
    fs::write(path, &data).map_err(|e| PeError::io(path, e))
}
