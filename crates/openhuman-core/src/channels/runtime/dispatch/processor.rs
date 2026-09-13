//! Core message processing loop for the channel runtime.
//!
//! Contains:
//! * [`channel_has_approval_surface`] — gate controlling per-channel approval
//!   context scoping.
//! * [`try_route_approval_reply`] (in [`approval`]) — intercepts yes/no
//!   approval replies before dispatching a fresh agent turn.
//! * [`process_channel_message`] — full per-message pipeline: typing, ACK
//!   reaction, history, agent turn, draft updates, reply.
//! * [`run_message_dispatch_loop`] — bounded-concurrency worker loop that feeds
//!   messages into [`process_channel_message`].

mod approval;
mod dispatch_loop;
mod message;
mod turn;

#[cfg(test)]
pub(crate) use approval::channel_has_approval_surface;
pub(crate) use dispatch_loop::run_message_dispatch_loop;
pub(crate) use message::RuntimeChannelMessage;
pub(crate) use turn::{process_channel_message, process_channel_runtime_message};
