use std::path::Path;

use crate::error::{Error, Result};

pub const DEFAULT_PUBLISHER: &str = "CN=LSW Self-Signed, O=LSW";

pub fn sign(path: &Path, publisher: Option<&str>) -> Result<()> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    if lsw_pe::detect(path)
        .map(|k| !matches!(k, lsw_pe::BinaryKind::Pe(_)))
        .unwrap_or(true)
    {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "only PE binaries can be Authenticode-signed".into(),
        });
    }
    let signed = path.with_extension("signed.tmp");
    crate::msixops::authenticode_sign(path, &signed, publisher.unwrap_or(DEFAULT_PUBLISHER))?;
    std::fs::rename(&signed, path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    Ok(())
}
