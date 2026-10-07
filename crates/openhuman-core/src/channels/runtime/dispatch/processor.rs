//! Core message processing loop for the channel runtime.
//!
//! Contains:
//! * [`channel_has_approval_surface`] — gate controlling per-channel approval
//!   context scoping.
//! * [`try_route_approval_reply`] (in [`approval`]) — intercepts yes/no
//!   approval replies before dispatching a fresh agent turn.
//! * [`process_channel_message`] — full per-message pipeline: typing, ACK
//!   reaction, history, agent turn, draft updates, reply.
//! * [`bus_turn`] — the orchestrator path: an unbound channel's turn over
//!   the native bus. A channel bound to a host agent runs through
//!   [`super::host_agent`] instead.
//! * [`run_message_dispatch_loop`] — feeds messages into
//!   [`process_channel_runtime_message`] through the bounded-concurrency
//!   `tinychannels::runtime::run_dispatch_loop`.

mod approval;
mod bus_turn;
mod turn;

use crate::channels::context::ChannelRuntimeContext;
use std::sync::Arc;

#[cfg(test)]
pub(crate) use approval::channel_has_approval_surface;
pub(crate) use tinychannels::runtime::RuntimeChannelMessage;
pub(crate) use turn::{process_channel_message, process_channel_runtime_message};

/// Run every inbound message through the agent pipeline, at most
/// `max_in_flight_messages` at a time, until `rx` closes.
pub(crate) async fn run_message_dispatch_loop(
    rx: tokio::sync::mpsc::Receiver<RuntimeChannelMessage>,
    ctx: Arc<ChannelRuntimeContext>,
    max_in_flight_messages: usize,
) {
    tinychannels::runtime::run_dispatch_loop(rx, max_in_flight_messages, move |msg| {
        process_channel_runtime_message(Arc::clone(&ctx), msg)
    })
    .await;
}
