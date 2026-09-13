//! `extra_metadata` side-channel keys on [`ChatMessage`]: turn usage /
//! provenance and tool-failure markers that the turn loop stamps before
//! persistence and the transcript writer lifts onto line fields.

use super::types::TurnUsage;
use crate::agent::messages::ChatMessage;

const TURN_USAGE_METADATA_KEY: &str = "openhuman_turn_usage";

/// `extra_metadata` key carrying a tool-result message's failure marker. The
/// harness folds a tool result into a `role:"tool"` message that drops the
/// per-call failure flag (`ToolResult::is_error`), so the turn loop re-attaches
/// the outcome here — from the captured `ToolCallOutcome` side-channel — before
/// persistence. `extra_metadata` is `#[serde(skip_serializing)]` on
/// [`ChatMessage`], so this never reaches the provider; the transcript writer
/// lifts it onto the additive [`MessageLine::failure`] / `failure_detail` line
/// fields and strips it from the persisted `extra_metadata`.
const TOOL_FAILURE_METADATA_KEY: &str = "openhuman_tool_failure";

/// Stamp a tool-result [`ChatMessage`] with its failure outcome so the
/// transcript writer can persist an explicit failure flag. `detail` is an
/// optional short, single-line reason (e.g. the head of the error output).
/// No-op semantics: pass this only for genuinely failed tool calls.
pub(crate) fn attach_tool_failure_metadata(message: &mut ChatMessage, detail: Option<&str>) {
    let mut payload = serde_json::Map::new();
    payload.insert("failure".to_string(), serde_json::Value::Bool(true));
    if let Some(detail) = detail.map(str::trim).filter(|s| !s.is_empty()) {
        payload.insert(
            "detail".to_string(),
            serde_json::Value::String(detail.to_string()),
        );
    }
    let marker = serde_json::Value::Object(payload);

    match message.extra_metadata.take() {
        Some(serde_json::Value::Object(mut map)) => {
            map.insert(TOOL_FAILURE_METADATA_KEY.to_string(), marker);
            message.extra_metadata = Some(serde_json::Value::Object(map));
        }
        Some(existing) => {
            let mut map = serde_json::Map::new();
            map.insert("value".to_string(), existing);
            map.insert(TOOL_FAILURE_METADATA_KEY.to_string(), marker);
            message.extra_metadata = Some(serde_json::Value::Object(map));
        }
        None => {
            let mut map = serde_json::Map::new();
            map.insert(TOOL_FAILURE_METADATA_KEY.to_string(), marker);
            message.extra_metadata = Some(serde_json::Value::Object(map));
        }
    }
}

/// Pop the tool-failure marker out of a cloned `extra_metadata` map, returning
/// `Some((true, detail))` when it was present. Strips the key so it is not
/// duplicated into the persisted `extra_metadata` alongside the top-level
/// `failure` line field. Legacy lines without the marker return `None`.
pub(super) fn take_tool_failure(
    extra: &mut Option<serde_json::Value>,
) -> Option<(bool, Option<String>)> {
    let serde_json::Value::Object(map) = extra.as_mut()? else {
        return None;
    };
    let marker = map.remove(TOOL_FAILURE_METADATA_KEY)?;
    // If removing the marker emptied the object, drop `extra_metadata` entirely
    // so a legacy-identical line stays legacy-identical.
    if map.is_empty() {
        *extra = None;
    }
    let detail = marker
        .get("detail")
        .and_then(|d| d.as_str())
        .map(str::to_string);
    Some((true, detail))
}

pub(crate) fn attach_turn_usage_metadata(message: &mut ChatMessage, turn_usage: &TurnUsage) {
    let Ok(payload) = serde_json::to_value(turn_usage) else {
        log::warn!("[transcript] failed to serialize turn usage metadata");
        return;
    };

    match message.extra_metadata.take() {
        Some(serde_json::Value::Object(mut map)) => {
            map.insert(TURN_USAGE_METADATA_KEY.to_string(), payload);
            message.extra_metadata = Some(serde_json::Value::Object(map));
        }
        Some(existing) => {
            let mut map = serde_json::Map::new();
            map.insert("value".to_string(), existing);
            map.insert(TURN_USAGE_METADATA_KEY.to_string(), payload);
            message.extra_metadata = Some(serde_json::Value::Object(map));
        }
        None => {
            let mut map = serde_json::Map::new();
            map.insert(TURN_USAGE_METADATA_KEY.to_string(), payload);
            message.extra_metadata = Some(serde_json::Value::Object(map));
        }
    }
}

pub(crate) fn turn_usage_extra_metadata(turn_usage: &TurnUsage) -> Option<serde_json::Value> {
    let mut message = ChatMessage::assistant("");
    attach_turn_usage_metadata(&mut message, turn_usage);
    message.extra_metadata
}

pub(super) fn turn_usage_from_metadata(message: &ChatMessage) -> Option<TurnUsage> {
    let payload = message
        .extra_metadata
        .as_ref()?
        .get(TURN_USAGE_METADATA_KEY)?;
    serde_json::from_value(payload.clone()).ok()
}
