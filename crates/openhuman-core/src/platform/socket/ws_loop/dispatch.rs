//! Incoming-message dispatch: Engine.IO framing and Socket.IO packet/ack
//! parsing.

use crate::platform::socket::manager::emit_state_change;
use crate::platform::socket::medulla::workflows;
use crate::platform::socket::types::ConnectionStatus;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::util::utf8_safe_prefix_at_byte_boundary;

use crate::platform::socket::event_handlers::{handle_sio_event, parse_sio_event};
use crate::platform::socket::manager::SharedState;

/// Handle an incoming Engine.IO text message by its type prefix.
pub(super) fn handle_eio_message(
    text: &str,
    emit_tx: &mpsc::UnboundedSender<String>,
    shared: &Arc<SharedState>,
) {
    if text.is_empty() {
        return;
    }

    match text.as_bytes()[0] {
        b'2' => {
            // Engine.IO PING → respond with PONG
            let _ = emit_tx.send("3".to_string());
        }
        b'3' => {
            // Engine.IO PONG — ignore (server responding to our ping)
        }
        b'4' => {
            // Engine.IO MESSAGE → contains Socket.IO packet
            if text.len() > 1 {
                handle_sio_packet(&text[1..], emit_tx, shared);
            }
        }
        b'1' => {
            log::info!("[socket] Engine.IO CLOSE from server");
        }
        b'6' => {
            // Engine.IO NOOP
        }
        _ => {
            log::debug!(
                "[socket] Unknown EIO packet: {}",
                utf8_safe_prefix_at_byte_boundary(text, 30)
            );
        }
    }
}

/// Handle a Socket.IO packet (after stripping the Engine.IO '4' prefix).
pub(super) fn handle_sio_packet(
    text: &str,
    emit_tx: &mpsc::UnboundedSender<String>,
    shared: &Arc<SharedState>,
) {
    if text.is_empty() {
        return;
    }

    match text.as_bytes()[0] {
        b'2' => {
            // Socket.IO EVENT: 2["eventName", data]
            if let Some((event_name, data)) = parse_sio_event(&text[1..]) {
                handle_sio_event(&event_name, data, emit_tx, shared);
            } else {
                log::warn!(
                    "[socket] Failed to parse SIO EVENT: {}",
                    utf8_safe_prefix_at_byte_boundary(text, 80)
                );
            }
        }
        b'3' => {
            // Socket.IO ACK: 3<ackId>[ackPayload]
            if let Some((ack_id, data)) = parse_sio_ack(&text[1..]) {
                if shared.ack_registry.resolve(ack_id, data) {
                    log::debug!("[socket] SIO ACK resolved ack_id={ack_id}");
                } else {
                    log::warn!("[socket] SIO ACK had no pending waiter ack_id={ack_id}");
                }
            } else {
                log::warn!(
                    "[socket] Failed to parse SIO ACK: {}",
                    utf8_safe_prefix_at_byte_boundary(text, 80)
                );
            }
        }
        b'0' => {
            // Socket.IO CONNECT (re-ack during reconnection) — update sid
            log::debug!("[socket] SIO CONNECT re-ack");
            if text.len() > 1 {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text[1..]) {
                    if let Some(sid) = data.get("sid").and_then(|v| v.as_str()) {
                        *shared.socket_id.write() = Some(sid.to_string());
                        emit_state_change(shared);
                    }
                }
            }
        }
        b'1' => {
            // Socket.IO DISCONNECT
            log::info!("[socket] SIO DISCONNECT from server");
            workflows::end_connection_generation();
            *shared.status.write() = ConnectionStatus::Disconnected;
            *shared.socket_id.write() = None;
            emit_state_change(shared);
        }
        b'4' => {
            // Socket.IO CONNECT_ERROR
            let error_str = if text.len() > 1 {
                &text[1..]
            } else {
                "unknown"
            };
            log::error!("[socket] SIO CONNECT_ERROR: {}", error_str);
        }
        _ => {
            log::debug!(
                "[socket] Unknown SIO packet type: {}",
                utf8_safe_prefix_at_byte_boundary(text, 30)
            );
        }
    }
}

pub(super) fn parse_sio_ack(text: &str) -> Option<(u64, serde_json::Value)> {
    let json_start = text.find('[')?;
    if json_start == 0 {
        return None;
    }
    let ack_id = text[..json_start].parse::<u64>().ok()?;
    let mut args: Vec<serde_json::Value> = serde_json::from_str(&text[json_start..]).ok()?;
    let data = if args.len() == 1 {
        args.pop().unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Array(args)
    };
    Some((ack_id, data))
}

// ---------------------------------------------------------------------------
// Redirect-following connect
// ---------------------------------------------------------------------------
