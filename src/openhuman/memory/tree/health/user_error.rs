//! The wire payload for the memory subsystem's `user_error` web-channel event.
//!
//! `tinymemory_core::tree::health::user_error` decides *that* a local-runtime
//! failure must reach the user; this module decides *what goes on the wire*.
//! The split is the same one the rest of the extraction follows — web channels,
//! and the socket the payload rides, are host surface.
//!
//! Both halves key on `LOCAL_MODEL_UNAVAILABLE_KIND`, which lives in the
//! contract crate so the two cannot drift apart silently.

use tinymemory_api::host::{
    LOCAL_MODEL_UNAVAILABLE_KIND, MEMORY_USER_ERROR_SOURCE, STORE_CORRUPT_KIND,
};

use crate::core::socketio::WebChannelEvent;

/// The metadata-only `user_error` payload for an unusable local embedding
/// runtime. Built separately from the publish so the no-leak contract is
/// unit-testable without a live socket.
///
/// Metadata only, exactly like the cron producer: a stable `kind` token in
/// `error_type` plus `error_source`, and never the raw provider text, the model
/// id, or the configured endpoint (which can carry a private host).
pub(crate) fn local_model_unavailable_user_error() -> WebChannelEvent {
    WebChannelEvent {
        event: "user_error".to_string(),
        // Every socket auto-joins the "system" room, so this reaches all
        // connected clients rather than one chat session.
        client_id: "system".to_string(),
        error_type: Some(LOCAL_MODEL_UNAVAILABLE_KIND.to_string()),
        error_source: Some(MEMORY_USER_ERROR_SOURCE.to_string()),
        ..Default::default()
    }
}

/// Broadcast the local-runtime user error to every connected client.
///
/// Called from the `MemoryEvent::LocalModelUnavailable` arm of the sink in
/// [`crate::openhuman::memory::host`]. `origin` names the producer
/// (`health_gate` / `embed_classify`) and is logged, never sent.
pub(crate) fn publish_local_model_unavailable_user_error(origin: &str) {
    log::debug!(
        "[memory::host] action=broadcast_user_error kind={LOCAL_MODEL_UNAVAILABLE_KIND} \
         source={MEMORY_USER_ERROR_SOURCE} origin={origin}"
    );
    crate::openhuman::web_chat::publish_web_channel_event(local_model_unavailable_user_error());
}

/// The metadata-only `user_error` payload for a corrupt-and-quarantined
/// memory-tree store (openhuman#5820). Same no-leak contract as the
/// local-model payload above: a stable `kind` token plus the source, never the
/// filesystem path (which is logged host-side instead).
pub(crate) fn store_corrupt_quarantined_user_error() -> WebChannelEvent {
    WebChannelEvent {
        event: "user_error".to_string(),
        // Every socket auto-joins the "system" room, so this reaches all
        // connected clients rather than one chat session.
        client_id: "system".to_string(),
        error_type: Some(STORE_CORRUPT_KIND.to_string()),
        error_source: Some(MEMORY_USER_ERROR_SOURCE.to_string()),
        ..Default::default()
    }
}

/// Text classifier for a corrupt chunk store, for errors that crossed the bus
/// and only exist as strings (a `MemoryError`'s rendered message).
///
/// The two phrases are SQLite's own renderings of `SQLITE_CORRUPT` (code 11)
/// and `SQLITE_NOTADB` (code 26) and survive every flattening the wire
/// applies. Mirrors the typed classifier inside `tinymemory-core` — the
/// module side classifies before the error is stringified; this is the
/// host-side fallback for paths that only ever see text.
pub(crate) fn is_corrupt_store_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("database disk image is malformed") || msg.contains("file is not a database")
}

/// Once-per-process corrupt-store notice for wire-text detectors
/// (openhuman#5820).
///
/// The archivist sees the corruption as one failed call per segment — 747
/// warns in the incident — so its escalation must not become 747 notices.
/// The engine's own quarantine still publishes the authoritative
/// [`MemoryEvent::StoreCorruptQuarantined`](tinymemory_api::host::MemoryEvent)
/// notice (un-latched, once per actual quarantine); this latch only bounds the
/// early-warning duplicate from paths that detect the damage before the
/// engine's recovery runs.
pub(crate) fn notice_corrupt_store_once(origin: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WIRE_CORRUPT_NOTICED: AtomicBool = AtomicBool::new(false);
    if WIRE_CORRUPT_NOTICED.swap(true, Ordering::Relaxed) {
        return;
    }
    publish_store_corrupt_user_error(origin, None);
}

/// Broadcast the corrupt-store user error and log the quarantined path
/// prominently (openhuman#5820 item 4 — 172 MB of a user's indexed memory
/// sitting on disk must never be a secret between the app and itself).
///
/// Called from the `MemoryEvent::StoreCorruptQuarantined` arms of both event
/// sinks — the in-process one in [`crate::openhuman::memory::host`] and the
/// module bridge in `crate::openhuman::modules::memory_host` — so the notice
/// fires whichever engine detected the damage. `origin` names the detecting
/// path; `quarantined_path` is the preserved copy, logged and never sent.
pub(crate) fn publish_store_corrupt_user_error(origin: &str, quarantined_path: Option<&str>) {
    match quarantined_path {
        Some(path) => log::error!(
            "[memory::host] the memory-tree store was corrupt and has been quarantined \
             (detected by {origin}). The damaged file is PRESERVED at {path} — recovery \
             tooling (e.g. `sqlite3 .recover`) can likely salvage most of it. The rebuilt \
             store is empty; re-sync memory sources to repopulate"
        ),
        None => log::error!(
            "[memory::host] the memory-tree store was corrupt and has been quarantined \
             (detected by {origin}); the damaged file is preserved beside the store as \
             chunks.db.corrupt-<timestamp>. The rebuilt store is empty; re-sync memory \
             sources to repopulate"
        ),
    }
    crate::openhuman::web_chat::publish_web_channel_event(store_corrupt_quarantined_user_error());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the wire shape the frontend `socketService` handler reads, plus the
    /// metadata-only no-leak contract.
    #[test]
    fn payload_is_metadata_only() {
        let event = local_model_unavailable_user_error();

        assert_eq!(event.event, "user_error");
        // The "system" room is the one every socket auto-joins.
        assert_eq!(event.client_id, "system");
        assert_eq!(
            event.error_type.as_deref(),
            Some(LOCAL_MODEL_UNAVAILABLE_KIND)
        );
        assert_eq!(
            event.error_source.as_deref(),
            Some(MEMORY_USER_ERROR_SOURCE)
        );

        // Nothing that could carry the base URL, a model id, or raw provider
        // prose may ride along.
        assert!(event.message.is_none(), "must not carry raw error prose");
        assert!(event.full_response.is_none());
        assert!(event.thread_id.is_empty());
    }

    /// The kind token is a cross-language contract: `app/src/types/userError.ts`
    /// declares this exact `UserErrorKind` discriminator and `classify.ts` keys
    /// on it. A rename on either side drops the signal with no compile error on
    /// either side, so pin the wire string.
    #[test]
    fn kind_matches_frontend_discriminator() {
        assert_eq!(LOCAL_MODEL_UNAVAILABLE_KIND, "local_model_unavailable");
    }

    /// `socketService` only maps `error_source == "memory"` onto the `memory`
    /// scope; anything else falls back to the historical `cron` default, which
    /// would file this entry under the wrong heading.
    #[test]
    fn source_matches_frontend_scope_mapping() {
        assert_eq!(MEMORY_USER_ERROR_SOURCE, "memory");
    }

    /// Same no-leak contract for the corrupt-store payload (openhuman#5820):
    /// the stable kind token and the memory source, never the quarantined
    /// path or SQLite prose.
    #[test]
    fn corrupt_store_payload_is_metadata_only() {
        let event = store_corrupt_quarantined_user_error();

        assert_eq!(event.event, "user_error");
        assert_eq!(event.client_id, "system");
        assert_eq!(event.error_type.as_deref(), Some(STORE_CORRUPT_KIND));
        assert_eq!(
            event.error_source.as_deref(),
            Some(MEMORY_USER_ERROR_SOURCE)
        );
        assert!(event.message.is_none(), "must not carry raw error prose");
        assert!(event.full_response.is_none());
        assert!(event.thread_id.is_empty());
    }

    /// The corrupt kind token is the same cross-language contract as the
    /// local-model one: `classify.ts` keys on exactly this string.
    #[test]
    fn corrupt_kind_matches_frontend_discriminator() {
        assert_eq!(STORE_CORRUPT_KIND, "memory_store_corrupt");
    }

    /// The wire-text classifier matches SQLite's two corruption renderings —
    /// the shapes a `MemoryError` string carries after crossing the bus — and
    /// nothing else. Quarantine-adjacent decisions key on this, so a false
    /// positive would raise a "memory quarantined" notice for a healthy store.
    #[test]
    fn corrupt_text_classifier_matches_sqlite_renderings_only() {
        assert!(is_corrupt_store_error(
            "memory-tree ingest failed for source `conversations:agent`: \
             database disk image is malformed"
        ));
        assert!(is_corrupt_store_error(
            "open failed: File is NOT a Database"
        ));
        assert!(!is_corrupt_store_error("database or disk is full"));
        assert!(!is_corrupt_store_error("rate limited (429)"));
        assert!(!is_corrupt_store_error(""));
    }

    /// The once-latch bounds the archivist's per-segment detection to one
    /// notice per process — 747 failing segments in the incident must not
    /// become 747 notices. (The engine's own quarantine event is un-latched
    /// and stays the authoritative per-quarantine notice.)
    #[test]
    fn wire_notice_is_latched_once_per_process() {
        // Publishing twice must be safe and quiet; the second call returns on
        // the latch. There is no socket in unit tests, so the observable
        // contract is "no panic, no double side effects on the latch path".
        notice_corrupt_store_once("test detector");
        notice_corrupt_store_once("test detector");
    }
}
