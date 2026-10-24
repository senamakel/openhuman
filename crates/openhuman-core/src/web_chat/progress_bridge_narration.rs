//! Interim-narration flushing and per-request event sequence stamping for the
//! progress bridge.

use crate::core::runtime::DispatchCtx;
use crate::web_chat::WebChannelEvent;

use super::super::event_bus::publish_web_channel_event_in;

/// Minimum trimmed length for the parent agent's leading narration to be
/// flushed as a `chat_interim` event when the round's first tool call starts.
///
/// Any non-empty narration flushes. This used to be 24 characters, meant to
/// keep a stray "Ok." from persisting as a bubble — but the frontend no longer
/// promotes narration to a message, and `chat_interim` is also the signal that
/// resets its live preview for the next round. A short "Let me check." that
/// was never flushed stayed in the preview and the next round's text was
/// appended straight onto it ("Let me check.Here's the answer").
pub(super) const MIN_INTERIM_NARRATION_CHARS: usize = 1;

/// Flush the parent agent's accumulated leading narration (streamed before a
/// tool call in the current round) as an interim `chat_interim` event, so it
/// persists as a chat bubble interleaved with the tool activity instead of
/// vanishing when the turn settles. Clears `buffer` unconditionally; emits
/// nothing for narration that is empty or too short to stand alone.
pub(super) fn flush_interim_narration(
    ctx: &crate::core::runtime::DispatchCtx,
    buffer: &mut String,
    round: u32,
    client_id: &str,
    thread_id: &str,
    request_id: &str,
    emit_seq: &mut u64,
) {
    let text = std::mem::take(buffer);
    let Some(narration) = interim_narration_text(&text) else {
        return;
    };
    log::debug!(
        "[web_channel][bridge] chat_interim round={} chars={} request_id={}",
        round,
        narration.chars().count(),
        request_id,
    );
    publish_seq_stamped(
        ctx,
        emit_seq,
        WebChannelEvent {
            event: "chat_interim".to_string(),
            client_id: client_id.to_string(),
            thread_id: thread_id.to_string(),
            request_id: request_id.to_string(),
            full_response: Some(narration),
            round: Some(round),
            ..Default::default()
        },
    );
}

/// Stamp a per-request monotonic sequence number on an outgoing web-channel
/// event and publish it. `request_id` is already carried on every
/// [`WebChannelEvent`]; `seq` is the additive ordering key the frontend uses to
/// dedup replayed vs live events by `(request_id, seq)` and to order them
/// identically to the persisted turn-state snapshot
/// (conversations-timeline-refactor, Phase 4). The counter advances once per
/// emitted event so each `(request_id, seq)` pair is unique within a turn.
pub(super) fn publish_seq_stamped(
    ctx: &DispatchCtx,
    next_seq: &mut u64,
    mut event: WebChannelEvent,
) {
    event.seq = Some(*next_seq);
    *next_seq = next_seq.saturating_add(1);
    publish_web_channel_event_in(ctx, event);
}

/// The trimmed narration to surface as an interim bubble, or `None` when it is
/// empty or shorter than [`MIN_INTERIM_NARRATION_CHARS`]. Pure so the threshold
/// is unit-testable without the global event bus.
pub(super) fn interim_narration_text(buffer: &str) -> Option<String> {
    let trimmed = buffer.trim();
    if trimmed.chars().count() < MIN_INTERIM_NARRATION_CHARS {
        return None;
    }
    Some(trimmed.to_string())
}
