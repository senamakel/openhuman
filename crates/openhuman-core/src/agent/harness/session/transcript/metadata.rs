//! `extra_metadata` side-channel keys on [`ChatMessage`]: turn usage /
//! provenance and tool-failure markers that the turn loop stamps before
//! persistence and the transcript writer lifts onto line fields.
//!
//! **The `openhuman_*` key namespace inside `extra_metadata` is reserved for
//! these host markers.** They travel in-band next to caller metadata, so the
//! writer strips a `openhuman_tool_failure` / `openhuman_replayed` key it finds
//! there whoever wrote it. A caller value stored under
//! [`WRAPPED_VALUE_KEY`] is safe, though: a wrap is recorded in the marker's own
//! payload rather than inferred from that key.

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

/// `extra_metadata` key marking a message **replayed from an earlier turn**: a
/// row read back from a transcript, or a message seeded from the conversation
/// log on cold boot. It carries the `request_id` the row was first written with
/// (`null` when it had none). The writer stamps a replayed row with that
/// original id instead of the current turn's, so resuming a thread into a fresh
/// transcript file does not re-attribute every earlier turn's rows to the
/// resuming request (#6282). Stripped from the persisted `extra_metadata`, like
/// the failure marker.
const REPLAYED_METADATA_KEY: &str = "openhuman_replayed";

/// Key a non-object `extra_metadata` value is moved under when a side-channel
/// marker has to be added next to it. The wrap is recorded in the marker's own
/// payload (`"wrapped": true`), never inferred from this key being present, so
/// caller metadata that happens to use the same key is left alone.
const WRAPPED_VALUE_KEY: &str = "openhuman_wrapped_value";

/// Field a marker payload carries when adding it wrapped a non-object
/// `extra_metadata` value that [`take_metadata`] must restore.
const WRAPPED_FLAG: &str = "wrapped";

/// Whether adding a marker to `message` would have to wrap its existing
/// `extra_metadata` (i.e. it is present and not an object).
fn would_wrap(message: &ChatMessage) -> bool {
    matches!(&message.extra_metadata, Some(value) if !value.is_object())
}

/// Insert `value` under `key` in `message.extra_metadata`, moving a non-object
/// value under [`WRAPPED_VALUE_KEY`] so nothing already there is lost. Callers
/// that need the value restored on removal record the wrap in their own payload
/// (see [`would_wrap`]).
fn insert_metadata(message: &mut ChatMessage, key: &str, value: serde_json::Value) {
    let mut map = match message.extra_metadata.take() {
        Some(serde_json::Value::Object(map)) => map,
        Some(existing) => {
            let mut map = serde_json::Map::new();
            map.insert(WRAPPED_VALUE_KEY.to_string(), existing);
            map
        }
        None => serde_json::Map::new(),
    };
    map.insert(key.to_string(), value);
    message.extra_metadata = Some(serde_json::Value::Object(map));
}

/// Pop `key` out of a cloned `extra_metadata` map, then undo what adding it
/// did: an object left empty becomes no `extra_metadata` (a legacy-identical
/// line stays legacy-identical), and a value this marker wrapped (its payload
/// says so) is restored, so a replayed or failed row persists its original
/// metadata exactly — including a caller object that itself uses
/// [`WRAPPED_VALUE_KEY`], which is never treated as a wrap.
fn take_metadata(extra: &mut Option<serde_json::Value>, key: &str) -> Option<serde_json::Value> {
    let serde_json::Value::Object(map) = extra.as_mut()? else {
        return None;
    };
    let value = map.remove(key)?;
    let wrapped = value
        .get(WRAPPED_FLAG)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if map.is_empty() {
        *extra = None;
    } else if wrapped {
        // Another marker may still sit beside the wrapped value (a replayed row
        // that also failed carries both). Restoring now would drop it, so hand
        // the wrap on and let whichever host marker is removed last restore.
        match [TOOL_FAILURE_METADATA_KEY, REPLAYED_METADATA_KEY]
            .into_iter()
            .find(|remaining| map.contains_key(*remaining))
        {
            Some(carrier) => {
                if let Some(serde_json::Value::Object(marker)) = map.get_mut(carrier) {
                    marker.insert(WRAPPED_FLAG.to_string(), serde_json::Value::Bool(true));
                }
            }
            // Only the wrapped value is left: restore it. Anything else still
            // there (turn usage) is not ours to strip, so the wrap stays.
            None if map.len() == 1 => {
                if let Some(original) = map.remove(WRAPPED_VALUE_KEY) {
                    *extra = Some(original);
                }
            }
            None => {}
        }
    }
    Some(value)
}

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
    if would_wrap(message) {
        payload.insert(WRAPPED_FLAG.to_string(), serde_json::Value::Bool(true));
    }
    insert_metadata(
        message,
        TOOL_FAILURE_METADATA_KEY,
        serde_json::Value::Object(payload),
    );
}

/// Pop the tool-failure marker out of a cloned `extra_metadata` map, returning
/// `Some((true, detail))` when it was present. Strips the key so it is not
/// duplicated into the persisted `extra_metadata` alongside the top-level
/// `failure` line field. Legacy lines without the marker return `None`.
pub(super) fn take_tool_failure(
    extra: &mut Option<serde_json::Value>,
) -> Option<(bool, Option<String>)> {
    let marker = take_metadata(extra, TOOL_FAILURE_METADATA_KEY)?;
    let detail = marker
        .get("detail")
        .and_then(|d| d.as_str())
        .map(str::to_string);
    Some((true, detail))
}

/// Mark `message` as replayed from an earlier turn whose request was
/// `request_id` (`None` when that turn recorded none). See
/// [`REPLAYED_METADATA_KEY`].
pub(crate) fn attach_replayed_metadata(message: &mut ChatMessage, request_id: Option<&str>) {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "request_id".to_string(),
        match request_id {
            Some(id) => serde_json::Value::String(id.to_string()),
            None => serde_json::Value::Null,
        },
    );
    if would_wrap(message) {
        payload.insert(WRAPPED_FLAG.to_string(), serde_json::Value::Bool(true));
    }
    insert_metadata(
        message,
        REPLAYED_METADATA_KEY,
        serde_json::Value::Object(payload),
    );
}

/// Mark `message` as replayed with no recorded request id, unless it already
/// carries a replayed marker. A transcript row read back with its own
/// `request_id` keeps that id; every other resumed row (a request-less line, a
/// conversation-log seed) must not take the resuming turn's id either (#6282).
pub(crate) fn mark_replayed_if_unmarked(message: &mut ChatMessage) {
    let marked = message
        .extra_metadata
        .as_ref()
        .and_then(|meta| meta.get(REPLAYED_METADATA_KEY))
        .is_some();
    if !marked {
        attach_replayed_metadata(message, None);
    }
}

/// Pop the replayed marker out of a cloned `extra_metadata` map, returning
/// `Some(original_request_id)` when the message was replayed and `None` when it
/// belongs to the turn being written.
pub(super) fn take_replayed_request_id(
    extra: &mut Option<serde_json::Value>,
) -> Option<Option<String>> {
    let marker = take_metadata(extra, REPLAYED_METADATA_KEY)?;
    Some(
        marker
            .get("request_id")
            .and_then(|id| id.as_str())
            .map(str::to_string),
    )
}

pub(crate) fn attach_turn_usage_metadata(message: &mut ChatMessage, turn_usage: &TurnUsage) {
    let Ok(payload) = serde_json::to_value(turn_usage) else {
        log::warn!("[transcript] failed to serialize turn usage metadata");
        return;
    };
    insert_metadata(message, TURN_USAGE_METADATA_KEY, payload);
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
