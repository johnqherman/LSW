use object::pe;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryKind {
    Pe(PeInfo),
    Elf,
    Script,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeInfo {
    pub format: PeFormat,
    pub machine: Machine,
    pub subsystem: Subsystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeFormat {
    Pe32,
    Pe32Plus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    X86,
    X86_64,
    Aarch64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subsystem {
    Console,
    Gui,
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
