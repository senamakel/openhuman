//! Host projection of a durable transcript row into a session-store journal
//! record.
//!
//! The importer, the live dual-write and the shadow read (all in
//! `tinyagents_session::transcript::import`) take this as their
//! `JournalProjector`. The journal record carries the row's typed sidecar
//! (turn usage, tool-failure and replay markers) as `openhuman_*` keys inside
//! `extra_metadata`; that key encoding is the journal's wire contract and is
//! derived here, in one direction only, from the typed fields of the vendor row.

use tinyagents_session::transcript::import::types::JournalMessage;
use tinyagents_session::transcript::TranscriptMessage;

const TURN_USAGE_METADATA_KEY: &str = "openhuman_turn_usage";
const TOOL_FAILURE_METADATA_KEY: &str = "openhuman_tool_failure";
const REPLAYED_METADATA_KEY: &str = "openhuman_replayed";
const WRAPPED_VALUE_KEY: &str = "openhuman_wrapped_value";
const WRAPPED_FLAG: &str = "wrapped";

fn would_wrap(extra: &Option<serde_json::Value>) -> bool {
    matches!(extra, Some(value) if !value.is_object())
}

/// Insert a marker into the row's metadata. A non-object original is kept
/// under [`WRAPPED_VALUE_KEY`] so no provider metadata is discarded.
fn insert_host_metadata(
    extra: &mut Option<serde_json::Value>,
    key: &str,
    value: serde_json::Value,
) {
    let mut map = match extra.take() {
        Some(serde_json::Value::Object(map)) => map,
        Some(existing) => {
            let mut map = serde_json::Map::new();
            map.insert(WRAPPED_VALUE_KEY.to_string(), existing);
            map
        }
        None => serde_json::Map::new(),
    };
    map.insert(key.to_string(), value);
    *extra = Some(serde_json::Value::Object(map));
}

/// The journal `extra_metadata` for one durable row.
fn journal_extra_metadata(message: &TranscriptMessage) -> Option<serde_json::Value> {
    let mut extra = message.extra_metadata.clone();
    if let Some(usage) = message.turn_usage.as_ref() {
        if let Ok(mut payload) = serde_json::to_value(usage) {
            // Older journal rows omitted these unknown measurements. Preserve
            // that wire shape while retaining measured final-call counts.
            if let Some(counts) = payload
                .get_mut("usage")
                .and_then(serde_json::Value::as_object_mut)
            {
                for field in ["last_call_input", "last_call_output"] {
                    if counts.get(field).and_then(serde_json::Value::as_u64) == Some(0) {
                        counts.remove(field);
                    }
                }
            }
            insert_host_metadata(&mut extra, TURN_USAGE_METADATA_KEY, payload);
        }
    }
    if let Some(failure) = message
        .tool_failure
        .as_ref()
        .filter(|failure| failure.failed)
    {
        let mut payload = serde_json::Map::new();
        payload.insert("failure".to_string(), serde_json::Value::Bool(true));
        if let Some(detail) = failure
            .detail
            .as_deref()
            .map(str::trim)
            .filter(|detail| !detail.is_empty())
        {
            payload.insert(
                "detail".to_string(),
                serde_json::Value::String(detail.to_string()),
            );
        }
        if would_wrap(&extra) {
            payload.insert(WRAPPED_FLAG.to_string(), serde_json::Value::Bool(true));
        }
        insert_host_metadata(
            &mut extra,
            TOOL_FAILURE_METADATA_KEY,
            serde_json::Value::Object(payload),
        );
    }
    if message.preserve_request_id {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "request_id".to_string(),
            message
                .request_id
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        if would_wrap(&extra) {
            payload.insert(WRAPPED_FLAG.to_string(), serde_json::Value::Bool(true));
        }
        insert_host_metadata(
            &mut extra,
            REPLAYED_METADATA_KEY,
            serde_json::Value::Object(payload),
        );
    }
    extra
}

/// The journal's established string form of a row: the native envelope for a
/// tool round, `[IMAGE:<url>]` markers for image parts, plain text otherwise.
fn journal_content(message: &TranscriptMessage) -> String {
    if message.parts.is_some() {
        message.display_content()
    } else {
        message.legacy_content()
    }
}

/// The `JournalProjector` OpenHuman passes to the importer.
pub fn journal_message_from_transcript(message: TranscriptMessage) -> JournalMessage {
    JournalMessage {
        id: message.id.clone(),
        role: message.role.clone(),
        content: journal_content(&message),
        extra_metadata: journal_extra_metadata(&message),
    }
}

#[cfg(test)]
#[path = "projector_tests.rs"]
mod tests;
