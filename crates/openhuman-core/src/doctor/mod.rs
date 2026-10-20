//! Diagnostic checks for OpenHuman configuration, workspace health, and daemon state.

mod contract;
mod core;
mod legacy;
pub mod ops;
pub mod registry;
mod runner;
mod schemas;
pub mod tools;

pub use contract::{
    ComponentId, DoctorCheck, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorReport,
    DoctorSummary, DoctorTier, Finding, FindingSeverity, ValueSource,
};
pub use core::{
    run, run_models, DiagnosticItem, MemoryEngineCheck, ModelProbeEntry, ModelProbeOutcome,
    ModelProbeReport, ModelProbeSummary, Severity,
};
pub use core::{DoctorReport as LegacyDoctorReport, DoctorSummary as LegacyDoctorSummary};
pub use ops as rpc;
pub use ops::*;
pub use registry::{register_check_source, register_check_source_in, DoctorCheckSource};
pub use schemas::{
    all_controller_schemas as all_doctor_controller_schemas,
    all_registered_controllers as all_doctor_registered_controllers,
};

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;

#[cfg(test)]
#[path = "checks/config_provenance_tests.rs"]
mod config_provenance_tests;

#[cfg(test)]
pub(crate) mod test_support;
