//! Core orchestration for LSW: build, run, test, package, and inspect
//! Windows applications from Linux using Wine and cross-compilation toolchains.

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

pub mod auditops;
pub mod buildops;
pub mod caseops;
pub mod checkops;
pub mod ciops;
pub mod compatdb;
pub mod compatops;
pub mod configops;
pub mod daemonops;
pub mod dapops;
pub(crate) mod dbgproxy;
pub mod debugops;
pub mod depsops;
pub mod diffops;
pub mod doctorops;
pub mod dotnetops;
pub mod dumpops;
pub(crate) mod dwarfline;
pub mod emulateops;
pub mod envops;
pub mod error;
pub mod explainops;
pub mod ideops;
pub mod inspectops;
pub mod installops;
pub mod msixops;
pub mod packageops;
pub mod pluginops;
pub mod project;
pub mod psops;
pub mod registryops;
pub mod reproops;
pub(crate) mod resourceops;
pub mod runops;
pub mod rustops;
pub mod sbomops;
pub mod sdkops;
pub mod serviceops;
pub mod setupops;
pub mod signops;
pub mod sizeops;
pub mod stringsops;
pub mod testops;
pub mod toolchainops;
pub mod traceops;
pub mod ttyops;
pub mod verifyops;
pub mod watchops;
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
