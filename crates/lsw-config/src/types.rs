use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetArch {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "x86")]
    X86,
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "armv7")]
    Armv7,
    #[serde(rename = "arm64ec")]
    Arm64Ec,
}

impl TargetArch {
    pub fn mingw_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-w64-mingw32",
            TargetArch::X86 => "i686-w64-mingw32",
            TargetArch::Aarch64 => "aarch64-w64-mingw32",
            TargetArch::Armv7 => "armv7-w64-mingw32",
            TargetArch::Arm64Ec => "arm64ec-w64-mingw32",
        }
    }

    pub fn msvc_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-pc-windows-msvc",
            TargetArch::X86 => "i686-pc-windows-msvc",
            TargetArch::Aarch64 => "aarch64-pc-windows-msvc",
            TargetArch::Armv7 => "thumbv7a-pc-windows-msvc",
            TargetArch::Arm64Ec => "arm64ec-pc-windows-msvc",
        }
    }

    pub fn msvc_lib_dirs(self) -> &'static [&'static str] {
        match self {
            TargetArch::X86_64 => &["x64", "x86_64"],
            TargetArch::X86 => &["x86"],
            TargetArch::Aarch64 | TargetArch::Arm64Ec => &["arm64", "aarch64"],
            TargetArch::Armv7 => &["arm", "armv7"],
        }
    }

    pub fn rust_gnu_triple(self) -> Option<&'static str> {
        match self {
            TargetArch::X86_64 => Some("x86_64-pc-windows-gnu"),
            TargetArch::X86 => Some("i686-pc-windows-gnu"),
            TargetArch::Aarch64 | TargetArch::Armv7 | TargetArch::Arm64Ec => None,
        }
    }
}

impl std::fmt::Display for TargetArch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TargetArch::X86_64 => "x86_64",
            TargetArch::X86 => "x86",
            TargetArch::Aarch64 => "aarch64",
            TargetArch::Armv7 => "armv7",
            TargetArch::Arm64Ec => "arm64ec",
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    #[default]
    Static,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaseSensitivity {
    #[default]
    Native,
    Strict,
}

impl CaseSensitivity {
    pub(crate) fn is_default(&self) -> bool {
        *self == CaseSensitivity::Native
    }
}
