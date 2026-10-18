//! Channel contracts and OpenHuman runtime orchestration.
//!
//! Provider orchestration is gated by `channels` and runs through the
//! TinyChannels TinyBus module. Serialized configuration, messages, and
//! schemas come from `tinychannels-bus`; OpenHuman retains host policy and
//! lifecycle orchestration.

// Always-compiled carve-outs (see module docs).
pub mod contract_schema;
#[cfg(any(feature = "voice", feature = "channels"))]
pub(crate) mod credentials;

pub use tinychannels_bus::{Channel, ChannelSendExt, SendMessage};

#[cfg(feature = "channels")]
pub mod bus;
#[cfg(feature = "channels")]
mod commands;
#[cfg(feature = "channels")]
pub(crate) use commands::{configured_providers, has_listener_configuration};
#[cfg(feature = "channels")]
pub(crate) mod context;
#[cfg(feature = "channels")]
pub mod controllers;
#[cfg(feature = "channels")]
pub mod host;
#[cfg(feature = "channels")]
pub mod proactive;
#[cfg(feature = "channels")]
pub mod providers;
#[cfg(feature = "channels")]
mod routes;
#[cfg(feature = "channels")]
mod runtime;
#[cfg(feature = "channels")]
pub(crate) mod system_prompt;

#[cfg(feature = "channels")]
pub(crate) use system_prompt::ChannelSystemPrompt;

#[cfg(all(feature = "channels", test))]
mod tests;

#[cfg(all(feature = "channels", any(test, debug_assertions)))]
pub use runtime::test_support;

#[cfg(feature = "channels")]
// Channel system-prompt assembly lives in
// `crate::agent::context::channels_prompt` alongside the rest of
// the prompt-building code. Re-exported here for callers that used the
// old `channels::build_system_prompt` path.
#[cfg(feature = "channels")]
pub use crate::agent::context::channels_prompt::build_system_prompt;
#[cfg(feature = "channels")]
pub use commands::doctor_channels;
#[cfg(feature = "channels")]
pub use runtime::start_channels;
#[cfg(feature = "channels")]
pub(crate) use runtime::{session, start_channels_with_session};

#[cfg(all(test, feature = "channels", feature = "modules"))]
#[path = "module_fixture_tests.rs"]
pub(crate) mod module_fixture;

#[cfg(test)]
#[path = "contract_reexport_tests.rs"]
mod contract_reexport_tests;
