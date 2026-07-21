pub mod buildops;
pub mod compatops;
pub mod daemonops;
pub mod debugops;
pub mod doctorops;
pub mod envops;
pub mod error;
pub mod ideops;
pub mod inspectops;
pub mod packageops;
pub mod pluginops;
pub mod project;
pub mod psops;
pub mod registryops;
pub mod runops;
pub mod rustops;
pub mod sdkops;
pub mod testops;
pub mod traceops;
pub mod verifyops;

pub use buildops::{BuildOptions, BuildReport, BuildSystem, build};
pub use doctorops::{DoctorReport, Section, Status, doctor};
pub use envops::{
    EnvCreateOptions, EnvCreateReport, EnvSummary, Environment, create as env_create,
    list as env_list, mapper, remove as env_remove, resolve_active, use_environment,
};
pub use error::{Error, Result};
pub use inspectops::{ImportStatus, InspectReport, inspect};
pub use project::{InitReport, Project, init};
pub use runops::{Display, Domain, RunReport, Sandbox, run, shell};
pub use testops::{CompatStatus, Outcome, TestOptions, TestReport, test};

pub use lsw_config::{Dirs, TargetArch};
