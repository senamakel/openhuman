//! Posting and progressively editing the evolving "draft" reply bubble.

use super::progressive_ui::{
    channel_edits_unsupported, classify_edit_failure, mark_channel_edits_unsupported, EditFailure,
    MAX_EDIT_FAILURES,
};
use super::streaming_state::StreamingState;
use serde_json::json;

/// Post or edit a draft message carrying the latest buffered text +
/// tool status. On the first call, sends a new message and records its
/// id; on subsequent calls, edits the existing message.
pub(super) async fn flush_streaming_edit(channel: &str, state: &mut StreamingState) {
    let draft = state.compose_draft();
    if draft.is_empty() {
        return;
    }
    state.dirty = false;

    let Some((client, jwt)) = super::delivery::build_channel_client().await else {
        return;
    };

    if let Some(ref message_id) = state.message_id {
        // Known-missing edit route: skip the guaranteed 404 and let
        // finalization replace the draft (delete + fresh atomic reply).
        if channel_edits_unsupported(channel) {
            tracing::debug!(
                "[channel-inbound][stream] skipping edit channel='{}' msg_id={} — no edit route on this backend, draft stays as-is until finalize",
                channel,
                message_id,
            );
            state.latch_draft_edits_unsupported();
            return;
        }
        let body = json!({ "text": draft });
        match client
            .send_channel_edit(channel, message_id, &jwt, body)
            .await
        {
            Ok(_) => {
                tracing::debug!(
                    "[channel-inbound][stream] edit ok channel='{}' msg_id={} chars={}",
                    channel,
                    message_id,
                    draft.len(),
                );
                state.edit_failures = 0;
            }
            Err(err) => {
                match classify_edit_failure(&err) {
                    EditFailure::RouteUnsupported => {
                        // Keep `message_id`: the draft is still on screen and
                        // finalization must be able to delete it before posting
                        // the canonical reply. Clearing it here (as the old
                        // `MessageNotFound` branch did) orphaned the draft and
                        // produced a stale "_working…_" bubble (#5230).
                        tracing::info!(
                            "[channel-inbound][stream] edit channel='{}' msg_id={} — backend has no edit route, keeping id for finalize cleanup and disabling progressive edits",
                            channel,
                            message_id,
                        );
                        mark_channel_edits_unsupported(channel);
                        state.latch_draft_edits_unsupported();
                        return;
                    }
                    EditFailure::MessageGone => {
                        tracing::info!(
                            "[channel-inbound][stream] edit channel='{}' msg_id={} — message gone provider-side (404), clearing stale id and disabling further edits",
                            channel,
                            message_id,
                        );
                        state.forget_draft();
                        return;
                    }
                    EditFailure::Transient => {}
                }
                state.edit_failures += 1;
                tracing::warn!(
                    "[channel-inbound][stream] edit failed channel='{}' msg_id={} err={} (failures={}/{})",
                    channel,
                    message_id,
                    err,
                    state.edit_failures,
                    MAX_EDIT_FAILURES,
                );
                if state.edit_failures >= MAX_EDIT_FAILURES {
                    tracing::info!(
                        "[channel-inbound][stream] giving up on progressive edits for channel='{}', falling back to atomic delivery",
                        channel,
                    );
                    state.edit_disabled = true;
                }
            }
        }
    } else {
        let body = super::delivery::channel_message_body_with_idempotency(
            channel,
            json!({ "text": draft }),
        );
        match client.send_channel_message(channel, &jwt, body).await {
            Ok(resp) => {
                // A message was posted to the user — record that fact
                // *before* checking for an id. Even if we can't extract
                // one (and thus can't edit it further), we must never
                // later fall back to sending a second atomic message.
                state.draft_sent = true;
                let id = extract_message_id(&resp);
                if let Some(id) = id {
                    tracing::debug!(
                        "[channel-inbound][stream] initial draft sent channel='{}' msg_id={}",
                        channel,
                        id,
                    );
                    state.message_id = Some(id);
                } else {
                    tracing::warn!(
                        "[channel-inbound][stream] initial draft sent but response lacked id — disabling progressive edits (finalize will skip sending a duplicate) channel='{}' resp={}",
                        channel,
                        resp,
                    );
                    state.edit_disabled = true;
                }
            }
            Err(err) => {
                state.edit_failures += 1;
                tracing::warn!(
                    "[channel-inbound][stream] initial send failed channel='{}' err={} (failures={})",
                    channel,
                    err,
                    state.edit_failures,
                );
                if state.edit_failures >= MAX_EDIT_FAILURES {
                    state.edit_disabled = true;
                }
            }
        }
    }
}

/// Extract a message id from a backend `send_channel_message` response.
/// The backend has used at least three shapes: `{"id":"..."}`,
/// `{"data":{"id":"..."}}`, and `{"messageId":1456,"success":true}` —
/// the last one returns the id as a JSON number, not a string, so
/// `as_str()` alone misses it (#600).
pub(super) fn extract_message_id(resp: &serde_json::Value) -> Option<String> {
    let candidate = resp
        .get("id")
        .or_else(|| resp.get("messageId"))
        .or_else(|| resp.get("data").and_then(|d| d.get("id")))
        .or_else(|| resp.get("data").and_then(|d| d.get("messageId")))?;
    if let Some(s) = candidate.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = candidate.as_i64() {
        return Some(n.to_string());
    }
    if let Some(n) = candidate.as_u64() {
        return Some(n.to_string());
    }
    None
}
