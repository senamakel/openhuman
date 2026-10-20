mod config_semantics;

use std::sync::Arc;

use super::contract::DoctorCheck;

/// Built-in native configuration checks, always available without a host
/// registering an extension source.
pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    config_semantics::checks()
}

#[cfg(test)]
#[path = "config_semantics_tests.rs"]
mod config_semantics_tests;
