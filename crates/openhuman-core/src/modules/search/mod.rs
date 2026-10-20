//! Generic TinySearch module transport: loading and serialized bus calls.
//!
//! OpenHuman's search configuration and profile policy live in
//! [`crate::search::module`]. This module owns only the generic adapter over
//! TinyBus.

mod proxy;

pub const MODULE_ID: &str = "tinysearch";

pub use proxy::{execute_tool, list_tools, refresh_loaded};

/// Record `configuration` as what the module is about to be loaded with.
pub(crate) fn record_load_configuration(configuration: &serde_json::Value) {
    proxy::record_load_configuration(configuration);
}

/// Load-time configuration for the generic module host.
pub(crate) fn load_configuration(config: &crate::config::Config) -> serde_json::Value {
    let configuration = serde_json::to_value(crate::search::module::module_config(config))
        .expect("TinySearch config serializes");
    record_load_configuration(&configuration);
    configuration
}
