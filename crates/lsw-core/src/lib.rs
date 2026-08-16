//! Core orchestration for LSW: build, run, test, package, and inspect
//! Windows applications from Linux using Wine and cross-compilation toolchains.

#![deny(missing_docs)]

pub(crate) fn sha256_file_checked(path: &std::path::Path) -> Result<String> {
    lsw_toolchain::sha256_file(path).map_err(|e| Error::io(path.to_path_buf(), e))
}

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(crate) fn diagnostic_stdio() -> std::process::Stdio {
    use std::os::fd::AsFd;
    std::io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .map_or_else(|_| std::process::Stdio::null(), std::process::Stdio::from)
}

/// PE security audit (ASLR, DEP, CFG, signing).
pub mod auditops;
/// Build orchestration across build systems.
pub mod buildops;
/// Case-sensitivity collision detection.
pub mod caseops;
/// Pre-flight project checks.
pub mod checkops;
/// CI configuration generation.
pub mod ciops;
/// Compatibility database persistence.
pub mod compatdb;
/// Windows compatibility queries.
pub mod compatops;
/// Project and environment configuration.
pub mod configops;
/// Debug Adapter Protocol (DAP) server.
pub mod dapops;
pub(crate) mod dbgproxy;
/// Debug session launcher.
pub mod debugops;
/// Dependency management and vendoring.
pub mod depsops;
/// PE binary diffing.
pub mod diffops;
/// System health diagnostics.
pub mod doctorops;
/// .NET / `NativeAOT` build support.
pub mod dotnetops;
/// Crash dump analysis.
pub mod dumpops;
pub(crate) mod dwarfline;
/// Wine process execution.
pub mod emulateops;
/// Environment lifecycle management.
pub mod envops;
/// Error types and result alias.
pub mod error;
/// Error code explanations.
pub mod explainops;
/// IDE integration (launch configs, `IntelliSense`).
pub mod ideops;
/// PE binary inspection.
pub mod inspectops;
/// Shell completion and man page installation.
pub mod installops;
/// MSIX package creation and signing.
pub mod msixops;
/// Installer packaging (MSI, NSIS, MSIX).
pub mod packageops;
pub mod pluginops;
/// Project discovery and initialization.
pub mod project;
/// Wine process listing.
pub mod psops;
/// Windows registry operations.
pub mod registryops;
/// Reproducible build verification.
pub mod reproops;
pub(crate) mod resourceops;
/// Process execution and interactive shell.
pub mod runops;
/// Rust cross-compilation support.
pub mod rustops;
/// SBOM generation.
pub mod sbomops;
/// Windows SDK import and management.
pub mod sdkops;
/// Windows service management.
pub mod serviceops;
/// Project scaffolding and setup.
pub mod setupops;
/// Code signing operations.
pub mod signops;
/// PE binary size analysis.
pub mod sizeops;
/// String extraction from binaries.
pub mod stringsops;
/// Test execution under Wine.
pub mod testops;
/// Toolchain installation and management.
pub mod toolchainops;
/// DLL load tracing.
pub mod traceops;
/// PTY and terminal handling.
pub mod ttyops;
/// Native Windows verification.
pub mod verifyops;
/// File watcher for rebuild-on-change.
pub mod watchops;
/// `WinRM` remote execution.
pub mod winrmops;

pub use buildops::{BuildOptions, BuildReport, BuildSystem, build};
pub use doctorops::{DoctorReport, Section, Status, doctor};
pub use envops::{
    EnvCreateOptions, EnvCreateReport, EnvSummary, Environment, clone_env, create as env_create,
    list as env_list, mapper, provision_winetricks, remove as env_remove, resolve_active,
    restore as env_restore, use_environment,
};
pub use error::{Error, Result};
pub use inspectops::{ImportStatus, InspectReport, inspect};
pub use project::{InitReport, Project, Template, init};
pub use runops::{Display, Domain, RunReport, Sandbox, run, shell};
pub use testops::{CompatStatus, Outcome, TestOptions, TestReport, test};

pub use lsw_config::{Dirs, TargetArch};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_plain_text_unchanged() {
        assert_eq!(xml_escape("hello world"), "hello world");
    }

    #[test]
    fn xml_escape_all_special_chars() {
        assert_eq!(
            xml_escape("<tag attr=\"val\" & 'q'>"),
            "&lt;tag attr=&quot;val&quot; &amp; &apos;q&apos;&gt;"
        );
    }

    #[test]
    fn xml_escape_empty_string() {
        assert_eq!(xml_escape(""), "");
    }

    #[test]
    fn xml_escape_only_ampersands() {
        assert_eq!(xml_escape("&&&"), "&amp;&amp;&amp;");
    }

    #[test]
    fn xml_escape_double_escape() {
        assert_eq!(xml_escape("&amp;"), "&amp;amp;");
    }

    #[test]
    fn xml_escape_unicode_preserved() {
        assert_eq!(xml_escape("caf\u{00e9} <\u{2603}>"), "caf\u{00e9} &lt;\u{2603}&gt;");
    }

    #[test]
    fn diagnostic_stdio_returns_valid_handle() {
        let stdio = diagnostic_stdio();
        let child = std::process::Command::new("true")
            .stderr(stdio)
            .output()
            .unwrap();
        assert!(child.status.success());
    }

    #[test]
    fn sha256_file_checked_nonexistent_path() {
        let err = sha256_file_checked(std::path::Path::new("/nonexistent/file.bin"));
        assert!(err.is_err());
    }
}
