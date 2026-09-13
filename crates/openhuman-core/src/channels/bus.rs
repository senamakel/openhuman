//! Event bus handlers for the channels domain.
//!
//! The [`ChannelInboundSubscriber`] handles inbound channel messages published
//! by the socket transport layer. It runs the agent inference loop via the web
//! channel provider and sends the reply back through the REST API.

mod delivery;
mod draft;
mod filler;
mod progressive_ui;
mod streaming_state;
mod subscriber;
mod thinking;
mod thread_id;

pub use subscriber::ChannelInboundSubscriber;

// Re-exported at module scope (rather than left as private `pub(super)`
// items reached only through their owning submodule) so the `#[path]`
// test modules below — which share this module's scope via `super::*` —
// can see them exactly as they could when this was one unsplit file.
use draft::extract_message_id;
use streaming_state::StreamingState;
use thinking::latest_thinking_snippet;
pub(crate) use thread_id::derive_inbound_thread_id;

#[cfg(test)]
#[path = "bus_inbound_thread_id_tests_tests.rs"]
mod inbound_thread_id_tests;

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;

#[cfg(any(test, debug_assertions))]
#[path = "bus_test_support_tests.rs"]
pub mod test_support;
