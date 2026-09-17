//! Rotating "still working" filler messages posted during long turns.

use super::progressive_ui::{MAX_FILLER_FAILURES, STATIC_FILLERS};
use super::streaming_state::StreamingState;
use super::thinking::latest_thinking_snippet;
use serde_json::json;

/// Post a fresh filler message to the channel and record its id so
/// `finalize_channel_reply` can delete it once the real response is on
/// screen. Prefers a live snippet of the agent's latest reasoning
/// (`thinking_accumulator`); falls back to the rotating `STATIC_FILLERS`
/// pool when there's no new thinking to show.
pub(super) async fn send_filler_message(channel: &str, state: &mut StreamingState) {
    let text = match latest_thinking_snippet(state) {
        Some(snippet) if state.last_filler_snippet.as_deref() != Some(snippet.as_str()) => {
            state.last_filler_snippet = Some(snippet.clone());
            format!("💭 _{snippet}…_")
        }
        _ => {
            let pool = STATIC_FILLERS;
            let idx = state.filler_index % pool.len();
            state.filler_index = state.filler_index.wrapping_add(1);
            pool[idx].to_string()
        }
    };

    let Some((client, jwt)) = super::delivery::build_channel_client().await else {
        return;
    };
    let body =
        super::delivery::channel_message_body_with_idempotency(channel, json!({ "text": text }));
    match client.send_channel_message(channel, &jwt, body).await {
        Ok(resp) => {
            state.filler_failures = 0;
            if let Some(id) = super::draft::extract_message_id(&resp) {
                tracing::debug!(
                    "[channel-inbound][filler] sent channel='{}' len={} msg_id={}",
                    channel,
                    text.len(),
                    id,
                );
                state.filler_message_ids.push(id);
            } else {
                tracing::warn!(
                    "[channel-inbound][filler] sent but response lacked id — cannot clean up on finalize channel='{}' resp={}",
                    channel,
                    resp,
                );
            }
        }
        Err(err) => {
            state.filler_failures = state.filler_failures.saturating_add(1);
            tracing::warn!(
                "[channel-inbound][filler] send failed channel='{}' err={} (failures={}/{})",
                channel,
                err,
                state.filler_failures,
                MAX_FILLER_FAILURES,
            );
            if state.filler_failures >= MAX_FILLER_FAILURES {
                tracing::info!(
                    "[channel-inbound][filler] disabling filler messages for channel='{}' — backend unsupported",
                    channel,
                );
                state.filler_disabled = true;
            }
        }
    }
}
