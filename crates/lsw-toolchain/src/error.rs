pub const LLVM_MINGW_ID: &str = "llvm-mingw";
pub const MINGW_GCC_ID: &str = "mingw-gcc";
pub const CLANG_CL_ID: &str = "clang-cl";

#[derive(Debug, thiserror::Error)]
pub enum ToolchainError {
    #[error(
        "LSW1401: toolchain provider '{id}' is unavailable: {detail}. \
         Possible fix: install the missing tool with your distribution's package manager"
    )]
    ProviderUnavailable { id: String, detail: String },

    #[error(
        "LSW1402: toolchain provider '{id}' failed its probe (could not produce a \
         working Windows PE binary): {detail}. Possible fix: reinstall the provider's \
         compiler and mingw-w64 sysroot, or pick another provider"
    )]
    ProbeFailed { id: String, detail: String },

    #[error(
        "LSW1403: no toolchain provider produced a working Windows PE binary:\n{}\n\
         Possible fixes: install mingw-w64 toolchain or clang+lld",
        format_attempts(attempts)
    )]
    NoWorkingProvider { attempts: Vec<(String, String)> },

    #[error(
        "LSW1404: unknown toolchain provider '{id}'. \
         Possible fix: use one of 'llvm-mingw' or 'mingw-gcc'"
    )]
    UnknownProvider { id: String },
}

fn format_attempts(attempts: &[(String, String)]) -> String {
    attempts
        .iter()
        .map(|(id, detail)| format!("  - {id}: {detail}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub provider: String,
    pub compiled: bool,
    pub linked: bool,
    pub produced_pe: bool,
    pub detail: String,
}

impl ProbeReport {
    pub fn passed(&self) -> bool {
        self.compiled && self.linked && self.produced_pe
    }
}
