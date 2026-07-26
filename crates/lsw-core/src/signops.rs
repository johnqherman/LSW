use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::msixops::SignIdentity;

const DEFAULT_PUBLISHER: &str = "CN=LSW Self-Signed, O=LSW";

#[derive(Debug, Default)]
pub struct SignOptions {
    pub publisher: Option<String>,
    pub pfx: Option<PathBuf>,
    pub pfx_pass_env: Option<String>,
    pub timestamp_url: Option<String>,
}

pub fn sign(path: &Path, opts: &SignOptions) -> Result<()> {
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

    let pass = match (&opts.pfx, &opts.pfx_pass_env) {
        (Some(_), Some(var)) => Some(std::env::var(var).map_err(|_| Error::MsixSign {
            detail: format!(
                "environment variable '{var}' is not set (it must hold the PFX passphrase)"
            ),
        })?),
        _ => None,
    };
    let identity = match &opts.pfx {
        Some(pfx) => SignIdentity::Pfx { path: pfx, pass },
        None => SignIdentity::DevCert {
            publisher: opts.publisher.as_deref().unwrap_or(DEFAULT_PUBLISHER),
        },
    };

    let signed = path.with_extension("signed.tmp");
    crate::msixops::authenticode_sign_with(
        path,
        &signed,
        &identity,
        opts.timestamp_url.as_deref(),
    )?;
    std::fs::rename(&signed, path).map_err(|e| Error::io(path.to_path_buf(), e))?;
    Ok(())
}
