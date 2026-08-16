use object::pe;

/// Detected binary format of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryKind {
    /// Windows PE executable or DLL.
    Pe(PeInfo),
    /// Linux ELF binary.
    Elf,
    /// Script with a `#!` shebang.
    Script,
    /// Unrecognized format.
    Unknown,
}

/// Core PE header fields: format, architecture, and subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeInfo {
    /// PE32 or PE32+ (64-bit).
    pub format: PeFormat,
    /// Target machine architecture.
    pub machine: Machine,
    /// Windows subsystem (console, GUI, etc.).
    pub subsystem: Subsystem,
}

/// PE optional header format variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeFormat {
    /// 32-bit PE.
    Pe32,
    /// 64-bit PE (PE32+).
    Pe32Plus,
}

/// Target machine architecture from the COFF header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    /// Intel x86 (32-bit).
    X86,
    /// AMD64 / x86-64.
    X86_64,
    /// ARM 64-bit (`AArch64`).
    Aarch64,
    /// Other architecture identified by raw COFF machine constant.
    Other(u16),
}

impl Machine {
    pub(crate) fn from_coff(value: u16) -> Self {
        match value {
            pe::IMAGE_FILE_MACHINE_I386 => Machine::X86,
            pe::IMAGE_FILE_MACHINE_AMD64 => Machine::X86_64,
            pe::IMAGE_FILE_MACHINE_ARM64 => Machine::Aarch64,
            other => Machine::Other(other),
        }
    }
}

/// Windows subsystem from the PE optional header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    /// Console (CUI) application.
    Console,
    /// Graphical (GUI) application.
    Gui,
    /// Other subsystem identified by raw constant.
    Other(u16),
}

impl Subsystem {
    pub(crate) fn from_pe(value: u16) -> Self {
        match value {
            pe::IMAGE_SUBSYSTEM_WINDOWS_GUI => Subsystem::Gui,
            pe::IMAGE_SUBSYSTEM_WINDOWS_CUI => Subsystem::Console,
            other => Subsystem::Other(other),
        }
    }
}
