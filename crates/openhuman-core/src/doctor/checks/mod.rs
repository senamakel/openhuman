//! Built-in doctor checks grouped by the config data they inspect.

mod config_file;

use std::sync::Arc;

use super::contract::DoctorCheck;

pub(super) fn builtins() -> Vec<Arc<dyn DoctorCheck>> {
    config_file::checks()
}
