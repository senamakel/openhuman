//! Built-in doctor checks grouped by configuration data and source.

mod config_file;
mod config_provenance;

use std::sync::Arc;

use super::contract::DoctorCheck;

pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    let mut checks = config_file::checks();
    checks.push(Arc::new(config_provenance::EnvironmentOverrideCheck));
    checks.push(Arc::new(config_provenance::ConfigSourceCheck));
    checks
}
