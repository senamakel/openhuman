//! Client-side executor for device-bound orchestration effects pushed by the
//! hosted brain over the socket.
//!
//! The backend computes a wake cycle and, when it wants the device to act,
//! delivers an effect frame (`orch:effect:send_dm`, `orch:effect:evict`, …). The
//! device runs the effect against its local Signal keys / memory and acks with
//! `orch:effect:result { callId, ok, error? }`. Delivery is at-least-once and the
//! client dedupes on `callId` (a `send_dm` whose ack was already latched
//! server-side is a no-op here — see [`is_duplicate_call`]).

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};

const LOG: &str = "orchestration";

/// A `send_dm` effect: relay `body` to `counterpartAgentId` over Signal, wrapping
/// it in `sessionId`'s session envelope so the peer threads the reply.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendDmEffect {
    #[serde(default)]
    pub cycle_id: String,
    pub call_id: String,
    pub counterpart_agent_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub body: String,
}

/// Parse an `orch:effect:send_dm` frame. Pure.
pub fn parse_send_dm(data: &Value) -> Result<SendDmEffect, String> {
    serde_json::from_value(data.clone()).map_err(|e| format!("parse send_dm: {e}"))
}

/// Build the `orch:effect:result` ack frame the device sends back. Pure.
pub fn effect_result_frame(call_id: &str, ok: bool, error: Option<&str>) -> Value {
    json!({ "callId": call_id, "ok": ok, "error": error })
}

/// The device-tool manifest declared to the hosted brain on socket connect
/// (`orch:register_tools`). These are **queryable** tools the reasoning loop may
/// call mid-cycle (results feed back), distinct from the terminal `send_dm`
/// effect. Phase 2 seeds it with a read-only status probe; local-workspace tools
/// grow this list as they are wired to the device tool dispatcher.
pub fn device_tool_manifest() -> Value {
    json!({
        "tools": [
            {
                "name": "device_status",
                "description": "Report this device's app version and platform.",
                "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        ]
    })
}

/// A device tool call pushed by the hosted brain (`orch:tool_call`). Run it
/// locally and return the result over `orch:tool_result`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallFrame {
    #[serde(default)]
    pub cycle_id: String,
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// Parse an `orch:tool_call` frame. Pure.
pub fn parse_tool_call(data: &Value) -> Result<ToolCallFrame, String> {
    serde_json::from_value(data.clone()).map_err(|e| format!("parse tool_call: {e}"))
}

/// Build the `orch:tool_result` frame returned to the hosted brain. Pure.
pub fn tool_result_frame(call_id: &str, ok: bool, result: Value, error: Option<&str>) -> Value {
    json!({ "callId": call_id, "ok": ok, "result": result, "error": error })
}

/// Run a device-declared tool locally. Read-only and side-effect-free for now;
/// local-workspace tools plug in here as they are added to the manifest.
pub fn dispatch_device_tool(name: &str, _args: &Value) -> Result<Value, String> {
    match name {
        "device_status" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
        })),
        other => Err(format!("unknown device tool: {other}")),
    }
}

/// Handle an inbound `orch:tool_call` frame end-to-end: parse → dispatch →
/// build the result frame. Returns `(callId, resultFrame)` to emit, or `None`
/// when the frame is unparseable.
pub fn handle_tool_call(data: &Value) -> Option<(String, Value)> {
    let frame = match parse_tool_call(data) {
        Ok(f) => f,
        Err(e) => {
            log::warn!(target: LOG, "[orchestration] tool_call.parse_failed: {e}");
            return None;
        }
    };
    let (ok, result, error) = match dispatch_device_tool(&frame.name, &frame.args) {
        Ok(value) => (true, value, None),
        Err(e) => (false, Value::Null, Some(e)),
    };
    Some((
        frame.call_id.clone(),
        tool_result_frame(&frame.call_id, ok, result, error.as_deref()),
    ))
}

// ── callId dedupe (at-least-once delivery guard) ──────────────────────────────

static SEEN_CALL_IDS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Record a `callId` and report whether it was already executed on this device.
/// Guards the at-least-once socket delivery so a redelivered effect is acked
/// (idempotently) but not executed twice.
pub fn is_duplicate_call(call_id: &str) -> bool {
    let mut guard = SEEN_CALL_IDS.lock().unwrap_or_else(|p| p.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    !set.insert(call_id.to_string())
}

/// Wrap the reply body for `session_id`. Empty/`master`/`subconscious` sessions
/// send the plain body; a real harness session is wrapped in a v1 envelope so
/// the peer threads it. Delegates to the shared orchestration helper.
fn outgoing_plaintext(session_id: &str, body: &str) -> Result<String, String> {
    if session_id.is_empty() {
        return Ok(body.to_string());
    }
    super::ops::session_send_plaintext(session_id, body).map_err(|e| e.to_string())
}

/// Execute a `send_dm` effect: wrap + send over Signal via the existing
/// tinyplace transport. The device's Signal keys never leave the machine.
pub async fn execute_send_dm(effect: &SendDmEffect) -> Result<(), String> {
    let plaintext = outgoing_plaintext(&effect.session_id, &effect.body)?;
    let mut params = serde_json::Map::new();
    params.insert(
        "recipient".to_string(),
        Value::from(effect.counterpart_agent_id.clone()),
    );
    params.insert("plaintext".to_string(), Value::from(plaintext));
    crate::openhuman::tinyplace::handle_tinyplace_signal_send_message(params)
        .await
        .map_err(|e| format!("signal send: {e}"))?;
    Ok(())
}

/// Handle an inbound `orch:effect:send_dm` frame end-to-end: parse → dedupe →
/// send → produce the ack frame. Returns `(callId, ackFrame)` for the caller to
/// emit, or `None` when the frame is unparseable (nothing to ack).
pub async fn handle_send_dm(data: &Value) -> Option<(String, Value)> {
    let effect = match parse_send_dm(data) {
        Ok(e) => e,
        Err(e) => {
            log::warn!(target: LOG, "[orchestration] effect.send_dm.parse_failed: {e}");
            return None;
        }
    };

    if is_duplicate_call(&effect.call_id) {
        log::debug!(
            target: LOG,
            "[orchestration] effect.send_dm.duplicate call_id={} (re-acking)",
            effect.call_id
        );
        return Some((
            effect.call_id.clone(),
            effect_result_frame(&effect.call_id, true, None),
        ));
    }

    let (ok, error) = match execute_send_dm(&effect).await {
        Ok(()) => {
            log::debug!(
                target: LOG,
                "[orchestration] effect.send_dm.sent call_id={} session={}",
                effect.call_id,
                effect.session_id
            );
            (true, None)
        }
        Err(e) => {
            log::warn!(target: LOG, "[orchestration] effect.send_dm.failed call_id={}: {e}", effect.call_id);
            (false, Some(e))
        }
    };
    Some((
        effect.call_id.clone(),
        effect_result_frame(&effect.call_id, ok, error.as_deref()),
    ))
}
