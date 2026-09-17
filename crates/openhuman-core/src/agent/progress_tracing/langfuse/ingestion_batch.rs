//! Shared ingestion-batch helpers: ISO timestamps, per-event ids, and the
//! 500-event chunking Langfuse imposes on a single ingestion request.

use serde_json::Value;

/// Epoch-milliseconds → RFC 3339 / ISO-8601 string (Langfuse requires ISO
/// timestamps, not epoch integers). Falls back to "now" only if the value is
/// somehow out of range — `start_unix_ms` comes from a monotonic wall clock so
/// this is defensive.
pub(super) fn iso_millis(unix_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(unix_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

/// Fresh per-event id. Langfuse dedupes ingestion events by this id, so it must
/// be unique per event (distinct from the observation/trace id in `body`).
pub(super) fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// is explicitly enabled.
/// Langfuse rejects an ingestion request whose `batch` holds more than 500
/// events (`400 "Langfuse ingestion batch cannot exceed 500 events"`). Large
/// turns — especially ones that spawn sub-agents — routinely exceed this.
pub(super) const LANGFUSE_MAX_BATCH_EVENTS: usize = 500;

/// Split a `{"batch": [...]}` ingestion payload into multiple payloads, each
/// carrying at most `max` events and preserving any other top-level keys.
///
/// Langfuse dedupes ingestion events by id and resolves each observation to its
/// trace by `traceId`, so delivering one run's events across several requests is
/// safe (the `trace-create` event stays in the first chunk). A payload at or
/// under the limit — or without a `batch` array — passes through unchanged as a
/// single element.
pub(super) fn split_ingestion_batch(payload: Value, max: usize) -> Vec<Value> {
    let events = match payload.get("batch").and_then(Value::as_array) {
        Some(events) if max > 0 && events.len() > max => events.clone(),
        _ => return vec![payload],
    };
    events
        .chunks(max)
        .map(|chunk| {
            let mut part = payload.clone();
            part["batch"] = Value::Array(chunk.to_vec());
            part
        })
        .collect()
}
