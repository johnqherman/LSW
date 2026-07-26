use std::path::{Path, PathBuf};

use crate::MZ_MAGIC;
use crate::error::PeError;

pub struct PeImage {
    pub(crate) path: PathBuf,
    pub(crate) data: Vec<u8>,
}

impl PeImage {
    pub fn open(path: &Path) -> Result<Self, PeError> {
        let data = crate::error::read_pe(path)?;
        if !data.starts_with(MZ_MAGIC) {
            return Err(PeError::NotPe {
                path: path.to_path_buf(),
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            data,
        })
    }
}

macro_rules! dispatch_pe {
    ($path:expr, $data:expr, $func:ident $(, $arg:expr)* $(,)?) => {{
        let path: &std::path::Path = $path;
        let data: &[u8] = $data;
        match object::read::pe::optional_header_magic(data)
            .map_err(|e| $crate::error::PeError::malformed(path, e))?
        {
            object::pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => {
                $func::<object::pe::ImageNtHeaders32>(path, data $(, $arg)*)
            }
            object::pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => {
                $func::<object::pe::ImageNtHeaders64>(path, data $(, $arg)*)
            }
            other => Err($crate::error::PeError::malformed(
                path,
                format!("unrecognized optional header magic 0x{other:04x}"),
            )),
        }
    }};
}

pub(crate) use dispatch_pe;
