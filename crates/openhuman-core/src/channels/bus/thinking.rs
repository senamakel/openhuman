//! Posting and progressively editing the ephemeral "💭 Thinking:" bubble.

use super::progressive_ui::{
    channel_edits_unsupported, classify_edit_failure, mark_channel_edits_unsupported, EditFailure,
    MAX_FILLER_CHARS, MAX_THINKING_DISPLAY_CHARS,
};
use super::streaming_state::StreamingState;
use serde_json::json;

/// Send or edit the ephemeral "💭 Thinking…" message on the channel.
/// This message is deleted when the final response is ready.
pub(super) async fn flush_thinking_message(channel: &str, state: &mut StreamingState) {
    state.thinking_dirty = false;

    if state.thinking_accumulator.trim().is_empty() {
        return;
    }

    let mut snippet = state.thinking_accumulator.trim().to_string();
    if snippet.len() > MAX_THINKING_DISPLAY_CHARS {
        snippet.truncate(MAX_THINKING_DISPLAY_CHARS);
        snippet.push('…');
    }
    let text = format!("💭 Thinking:\n_{snippet}_");

    let Some((client, jwt)) = super::delivery::build_channel_client().await else {
        return;
    };

    if let Some(msg_id) = state.thinking_message_id.clone() {
        // Known-missing edit route: the bubble stays as first posted and is
        // deleted at finalization. Skip the guaranteed 404.
        if channel_edits_unsupported(channel) {
            tracing::debug!(
                "[channel-inbound][thinking] skipping edit channel='{}' msg_id={} — no edit route on this backend, bubble stays until finalize deletes it",
                channel,
                msg_id,
            );
            state.latch_thinking_edits_unsupported();
            return;
        }
        // Edit existing thinking message with updated content.
        let body = json!({ "text": text });
        if let Err(err) = client.send_channel_edit(channel, &msg_id, &jwt, body).await {
            match classify_edit_failure(&err) {
                EditFailure::RouteUnsupported => {
                    // Keep `thinking_message_id`. Clearing it (as the old
                    // `MessageNotFound` branch did) meant finalization had
                    // nothing to delete, so the ephemeral "💭 Thinking:"
                    // bubble stayed in the chat forever (#5230).
                    tracing::info!(
                        "[channel-inbound][thinking] edit channel='{}' msg_id={} — backend has no edit route, keeping id so finalize still deletes the bubble",
                        channel,
                        msg_id,
                    );
                    mark_channel_edits_unsupported(channel);
                    state.latch_thinking_edits_unsupported();
                }
                EditFailure::MessageGone => {
                    tracing::info!(
                        "[channel-inbound][thinking] edit channel='{}' msg_id={} — thinking msg gone provider-side (404), clearing id and disabling further thinking edits",
                        channel,
                        msg_id,
                    );
                    state.forget_thinking();
                }
                EditFailure::Transient => {
                    tracing::debug!(
                        "[channel-inbound][thinking] edit failed channel='{}' msg_id={} err={}",
                        channel,
                        msg_id,
                        err,
                    );
                }
            }
        }
    } else {
        // Send initial thinking message.
        let body = super::delivery::channel_message_body_with_idempotency(
            channel,
            json!({ "text": text }),
        );
        match client.send_channel_message(channel, &jwt, body).await {
            Ok(resp) => {
                state.thinking_sent = true;
                let id = super::draft::extract_message_id(&resp);
                if let Some(id) = id {
                    tracing::debug!(
                        "[channel-inbound][thinking] thinking msg sent channel='{}' msg_id={}",
                        channel,
                        id,
                    );
                    state.thinking_message_id = Some(id);
                } else {
                    tracing::warn!(
                        "[channel-inbound][thinking] thinking msg sent but response lacked id — disabling further thinking flushes (message won't be deletable) channel='{}' resp={}",
                        channel,
                        resp,
                    );
                    state.thinking_edit_disabled = true;
                }
            }
            Err(err) => {
                tracing::warn!(
                    "[channel-inbound][thinking] failed to send thinking msg channel='{}' err={} — disabling further thinking flushes",
                    channel,
                    err,
                );
                state.thinking_edit_disabled = true;
            }
        }
    }
}

/// Pull the most recent `MAX_FILLER_CHARS` Unicode scalars out of the
/// thinking accumulator so we can surface a live snapshot of the agent's
/// reasoning as a filler. Returns `None` when there's nothing to show
/// yet. Trims any partial leading word so the snippet reads cleanly.
pub(super) fn latest_thinking_snippet(state: &StreamingState) -> Option<String> {
    let acc = state.thinking_accumulator.trim();
    if acc.is_empty() {
        return None;
    }
    let total = acc.chars().count();
    let snippet: String = if total <= MAX_FILLER_CHARS {
        acc.to_string()
    } else {
        acc.chars().skip(total - MAX_FILLER_CHARS).collect()
    };
    let trimmed = snippet
        .trim_start_matches(|c: char| !c.is_whitespace())
        .trim_start()
        .to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
