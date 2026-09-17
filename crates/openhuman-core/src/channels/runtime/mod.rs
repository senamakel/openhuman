//! Channel runtime entry points.

mod dispatch;
pub(crate) mod session;
mod startup;
mod supervision;

pub use startup::start_channels;
pub(crate) use startup::start_channels_with_session;

#[cfg(any(test, debug_assertions))]
pub mod test_support;

// Re-exported for `channels::tests` only; omit in normal lib builds to avoid unused-import warnings.
#[cfg(test)]
pub(crate) use dispatch::{
    process_channel_message, run_message_dispatch_loop, RuntimeChannelMessage,
};
#[cfg(test)]
pub(crate) use supervision::spawn_supervised_listener;
