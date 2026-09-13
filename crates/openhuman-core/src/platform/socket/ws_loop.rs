//! WebSocket Engine.IO / Socket.IO connection loop with automatic reconnection.
//!
//! Split by responsibility rather than by line count:
//! - [`dispatch`] parses incoming Engine.IO/Socket.IO frames.
//! - [`connect`] runs the redirect-following connect and one connection's
//!   handshake and event loop.
//! - [`reconnect`] is the outer [`ws_loop`] retry/backoff loop that drives
//!   [`connect::run_connection`] and decides how to react to failures.

#[cfg(test)]
#[path = "ws_loop_tests.rs"]
mod tests;

mod connect;
mod dispatch;
mod reconnect;

pub(super) use reconnect::ws_loop;

#[cfg(test)]
use connect::{
    connect_with_redirects, extract_location_header, is_redirect_status, record_redirect_warning,
    resolve_redirect_target,
};
#[cfg(test)]
use dispatch::{handle_eio_message, handle_sio_packet, parse_sio_ack};
#[cfg(test)]
use reconnect::{
    decide_after_invalid_token, drain_pending_emits, log_connection_failure, InvalidTokenAction,
    FAIL_ESCALATE_THRESHOLD,
};

#[cfg(test)]
use super::manager::AckRegistry;
