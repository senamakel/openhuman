//! Core channel connect/disconnect/status operations.

mod catalog;
mod connect_channel;
mod disconnect;
mod email;
mod memory;
pub(super) mod shared;
mod status;
mod test_channel;

#[allow(unused_imports)]
pub(crate) use catalog::{describe_channel, list_channels};
pub use connect_channel::connect_channel;
pub use disconnect::disconnect_channel;
#[cfg(test)]
pub(crate) use email::persist_email_config;
#[cfg(test)]
pub(crate) use shared::merge_listener_health;
pub use status::{
    channel_status, connected_channel_slugs, get_default_channel, set_default_channel,
};
pub use test_channel::test_channel;

// `email_config_tests` below shares this module's scope via `use super::*;` —
// the same way it did when this was one unsplit file — so bring in the
// symbols it (and any sibling test module) reaches that way.
#[cfg(test)]
use crate::channels::email_channel::EmailConfig;
#[cfg(test)]
use email::{build_email_config, parse_email_senders, parse_port_field};
#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
#[path = "connect_email_config_tests_tests.rs"]
mod email_config_tests;
