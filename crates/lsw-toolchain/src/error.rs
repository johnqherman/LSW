/// Provider identifier for LLVM MinGW.
pub const LLVM_MINGW_ID: &str = "llvm-mingw";
/// Provider identifier for MinGW GCC.
pub const MINGW_GCC_ID: &str = "mingw-gcc";
/// Provider identifier for clang-cl (MSVC-ABI).
pub const CLANG_CL_ID: &str = "clang-cl";

/// Errors from toolchain resolution, probing, and selection.
#[derive(Debug, thiserror::Error)]
pub enum ToolchainError {
    #[error(
        "LSW1401: toolchain provider '{id}' is unavailable: {detail}. \
         Possible fix: install the missing tool with your distribution's package manager"
    )]
    /// Named provider is not installed or not on PATH.
    ProviderUnavailable {
        /// Provider identifier.
        id: String,
        /// Why it is unavailable.
        detail: String,
    },

    #[error(
        "LSW1402: toolchain provider '{id}' failed its probe (could not produce a \
         working Windows PE binary): {detail}. Possible fix: reinstall the provider's \
         compiler and mingw-w64 sysroot, or pick another provider"
    )]
    /// Provider resolved but failed its compile-link-detect probe.
    ProbeFailed {
        /// Provider identifier.
        id: String,
        /// Probe failure detail.
        detail: String,
    },

    #[error(
        "LSW1403: no toolchain provider produced a working Windows PE binary:\n{}\n\
         Possible fixes: install mingw-w64 toolchain or clang+lld",
        format_attempts(attempts)
    )]
    /// All providers were tried and none produced a working PE.
    NoWorkingProvider {
        /// Per-provider failure details.
        attempts: Vec<(String, String)>,
    },

    #[error(
        "LSW1404: unknown toolchain provider '{id}'. \
         Possible fix: use one of 'llvm-mingw' or 'mingw-gcc'"
    )]
    /// Requested provider name is not recognized.
    UnknownProvider {
        /// Provider identifier.
        id: String,
    },
}

fn format_attempts(attempts: &[(String, String)]) -> String {
    attempts
        .iter()
        .map(|(id, detail)| format!("  - {id}: {detail}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result of probing a toolchain provider with a test compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// Provider identifier that was probed.
    pub provider: String,
    /// Whether compilation succeeded.
    pub compiled: bool,
    /// Whether linking succeeded.
    pub linked: bool,
    /// Whether the output was a valid Windows PE.
    pub produced_pe: bool,
    /// Human-readable status or error message.
    pub detail: String,
}

impl ProbeReport {
    pub(crate) fn failure(provider: &str, detail: String, compiled: bool) -> Self {
        Self {
            provider: provider.to_owned(),
            compiled,
            linked: false,
            produced_pe: false,
            detail,
        }
    }
}

impl ProbeReport {
    /// Returns true if the probe compiled, linked, and produced a valid PE.
    pub fn passed(&self) -> bool {
        self.compiled && self.linked && self.produced_pe
    }
}
