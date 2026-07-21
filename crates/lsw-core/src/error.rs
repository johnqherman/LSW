use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] lsw_config::ConfigError),
    #[error(transparent)]
    Path(#[from] lsw_path::PathError),
    #[error(transparent)]
    Pe(#[from] lsw_pe::PeError),
    #[error(transparent)]
    Toolchain(#[from] lsw_toolchain::ToolchainError),
    #[error(transparent)]
    Runtime(#[from] lsw_runtime::RuntimeError),

    #[error(
        "LSW2001: no active environment for this project\n\
         Possible fixes:\n  lsw env create <name>\n  lsw use <name>"
    )]
    NoActiveEnvironment,

    #[error(
        "LSW2002: environment '{name}' does not exist\n\
         Possible fixes:\n  lsw env create {name}\n  lsw env list"
    )]
    EnvironmentNotFound { name: String },

    #[error(
        "LSW2003: environment '{name}' already exists\n\
         Possible fixes:\n  lsw env remove {name}\n  choose another name"
    )]
    EnvironmentExists { name: String },

    #[error("LSW2004: '{}' is not something LSW can execute: {detail}", program.display())]
    NotExecutable { program: PathBuf, detail: String },

    #[error(
        "LSW2005: build failed (exit code {code:?})\n\
         Command: {command}"
    )]
    BuildFailed { command: String, code: Option<i32> },

    #[error(
        "LSW2006: lsw.lock does not match environment '{environment}'\n\
         {detail}\n\
         Possible fixes:\n  lsw build --update-lock\n  lsw env remove {environment} && lsw env create {environment}"
    )]
    LockMismatch { environment: String, detail: String },

    #[error(
        "LSW2007: no build system detected\n\
         Expected CMakeLists.txt or a [build] command in lsw.toml"
    )]
    NoBuildSystem,

    #[error("LSW2008: target os '{os}' is not supported (only 'windows')")]
    UnsupportedTargetOs { os: String },

    #[error("LSW2009: cannot create project at {}: {detail}", path.display())]
    InitFailed { path: PathBuf, detail: String },

    #[error("LSW2010: io error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("LSW2011: required tool '{tool}' not found on PATH\nPossible fixes: {fix}")]
    ToolMissing { tool: String, fix: String },

    #[error(
        "LSW2012: invalid {kind} name '{name}'\n\
         Names must be non-empty and must not contain path separators, '..', or NUL"
    )]
    InvalidName { kind: String, name: String },

    #[error(
        "LSW2015: registry operation failed (exit code {code:?})\n\
         Check the key path (e.g. 'HKCU\\Software\\Example\\App') and see the output above"
    )]
    RegistryOperationFailed { code: Option<i32> },

    #[error(
        "LSW2014: nothing to test\n\
         Possible fixes:\n  \
         add add_test(...) to CMakeLists.txt and rebuild, or\n  \
         set [test].command in lsw.toml"
    )]
    NoTests,

    #[error(
        "LSW2013: build produced '{}' which is not a Windows PE binary ({found})\n\
         The build ran with host tools but did not cross-compile.\n\
         Possible fixes:\n  \
         use the generated CMake toolchain (default `lsw build`), or\n  \
         make your [build] command honor CC/CXX/CFLAGS/CXXFLAGS/LDFLAGS", artifact.display()
    )]
    ArtifactNotPe { artifact: PathBuf, found: String },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }

    pub fn code(&self) -> String {
        let text = self.to_string();
        text.split(':')
            .next()
            .filter(|head| head.starts_with("LSW"))
            .unwrap_or("LSW0000")
            .to_owned()
    }
}

pub type Result<T> = std::result::Result<T, Error>;
