//! Built-in doctor checks grouped by the configuration area they inspect.

mod config_provenance;

use std::sync::Arc;

use super::contract::DoctorCheck;

pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    vec![
        Arc::new(config_provenance::EnvironmentOverrideCheck),
        Arc::new(config_provenance::ConfigSourceCheck),
    ]
}
