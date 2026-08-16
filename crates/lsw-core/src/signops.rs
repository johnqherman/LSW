use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::msixops::SignIdentity;

const DEFAULT_PUBLISHER: &str = "CN=LSW Self-Signed, O=LSW";

#[derive(Debug, Default)]
/// Sign Options.
pub struct SignOptions {
    /// Publisher.
    pub publisher: Option<String>,
    /// Pfx.
    pub pfx: Option<PathBuf>,
    /// Pfx pass env.
    pub pfx_pass_env: Option<String>,
    /// Timestamp url.
    pub timestamp_url: Option<String>,
}

#[derive(Debug)]
/// Verify Outcome.
pub struct VerifyOutcome {
    /// Valid.
    pub valid: bool,
    /// Detail.
    pub detail: String,
}

/// Verify signature.
pub fn verify_signature(path: &Path) -> Result<VerifyOutcome> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    let Some(tool) = crate::buildops::which("osslsigncode") else {
        return Err(Error::ToolMissing {
            tool: "osslsigncode".into(),
            fix: "install osslsigncode to verify Authenticode signatures".into(),
        });
    };
    let out = std::process::Command::new(&tool)
        .arg("verify")
        .arg(path)
        .output()
        .map_err(|e| Error::io(tool.clone(), e))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let valid = out.status.success() && stdout.contains("Signature verification: ok");
    let detail = stdout
        .lines()
        .filter(|l| {
            l.contains("Signature verification")
                || l.contains("Subject:")
                || l.contains("Issuer :")
                || l.contains("no signature found")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(VerifyOutcome {
        valid,
        detail: if detail.is_empty() {
            stdout.trim().to_owned()
        } else {
            detail
        },
    })
}

/// Sign.
pub fn sign(path: &Path, opts: &SignOptions) -> Result<()> {
    if !path.is_file() {
        return Err(Error::NotExecutable {
            program: path.to_path_buf(),
            detail: "file not found".into(),
        });
    }
    if lsw_pe::detect(path).map_or(true, |k| !matches!(k, lsw_pe::BinaryKind::Pe(_))) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_options_default_has_no_publisher() {
        let opts = SignOptions::default();
        assert!(opts.publisher.is_none());
        assert!(opts.pfx.is_none());
        assert!(opts.pfx_pass_env.is_none());
        assert!(opts.timestamp_url.is_none());
    }

    #[test]
    fn default_publisher_is_lsw_self_signed() {
        assert!(DEFAULT_PUBLISHER.contains("LSW Self-Signed"));
        assert!(DEFAULT_PUBLISHER.starts_with("CN="));
    }

    #[test]
    fn verify_signature_nonexistent_file_returns_error() {
        let result = verify_signature(Path::new("/nonexistent/binary.exe"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotExecutable { .. }));
    }

    #[test]
    fn sign_nonexistent_file_returns_error() {
        let result = sign(
            Path::new("/nonexistent/binary.exe"),
            &SignOptions::default(),
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotExecutable { .. }));
    }

    #[test]
    fn sign_rejects_non_pe_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"not a PE file").unwrap();
        let result = sign(tmp.path(), &SignOptions::default());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::NotExecutable { .. }));
    }
}
