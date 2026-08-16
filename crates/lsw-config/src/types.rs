use serde::{Deserialize, Serialize};

/// Windows target CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetArch {
    /// 64-bit x86.
    #[serde(rename = "x86_64")]
    X86_64,
    /// 32-bit x86.
    #[serde(rename = "x86")]
    X86,
    /// 64-bit ARM.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// 32-bit ARM (Thumb-2).
    #[serde(rename = "armv7")]
    Armv7,
    /// ARM64EC (emulation-compatible ABI).
    #[serde(rename = "arm64ec")]
    Arm64Ec,
}

impl TargetArch {
    /// ```
    /// use lsw_config::TargetArch;
    /// assert_eq!(TargetArch::X86_64.mingw_triple(), "x86_64-w64-mingw32");
    /// ```
    pub fn mingw_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-w64-mingw32",
            TargetArch::X86 => "i686-w64-mingw32",
            TargetArch::Aarch64 => "aarch64-w64-mingw32",
            TargetArch::Armv7 => "armv7-w64-mingw32",
            TargetArch::Arm64Ec => "arm64ec-w64-mingw32",
        }
    }

    /// Returns the MSVC target triple for this architecture.
    pub fn msvc_triple(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x86_64-pc-windows-msvc",
            TargetArch::X86 => "i686-pc-windows-msvc",
            TargetArch::Aarch64 => "aarch64-pc-windows-msvc",
            TargetArch::Armv7 => "thumbv7a-pc-windows-msvc",
            TargetArch::Arm64Ec => "arm64ec-pc-windows-msvc",
        }
    }

    /// ```
    /// use lsw_config::TargetArch;
    /// assert_eq!(TargetArch::X86_64.win_arch_name(), "x64");
    /// assert_eq!(TargetArch::X86.win_arch_name(), "x86");
    /// ```
    pub fn win_arch_name(self) -> &'static str {
        match self {
            TargetArch::X86_64 => "x64",
            TargetArch::X86 => "x86",
            TargetArch::Aarch64 | TargetArch::Arm64Ec => "arm64",
            TargetArch::Armv7 => "arm",
        }
    }

    /// Returns MSVC SDK library directory name candidates.
    pub fn msvc_lib_dirs(self) -> &'static [&'static str] {
        match self {
            TargetArch::X86_64 => &["x64", "x86_64"],
            TargetArch::X86 => &["x86"],
            TargetArch::Aarch64 | TargetArch::Arm64Ec => &["arm64", "aarch64"],
            TargetArch::Armv7 => &["arm", "armv7"],
        }
    }

    /// Returns the Rust GNU-ABI triple, if one exists for this architecture.
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

/// Static vs dynamic linking preference for the cross toolchain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    /// Prefer static linking (default).
    #[default]
    Static,
    /// Prefer dynamic linking.
    Dynamic,
}

/// Filesystem case-sensitivity mode for cross-compilation validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaseSensitivity {
    /// Use the host filesystem's native behavior (default).
    #[default]
    Native,
    /// Enforce strict case-insensitive collision checks.
    Strict,
}

impl CaseSensitivity {
    pub(crate) fn is_default(&self) -> bool {
        *self == CaseSensitivity::Native
    }
}
