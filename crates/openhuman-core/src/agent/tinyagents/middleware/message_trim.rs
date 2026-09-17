//! [`ImageAwareMessageTrimMiddleware`] and the image-aware token estimation it
//! (and the wrap-up / contents-list middlewares) budget with (issue #4462).

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyinference::message::{ContentBlock, Message as TaMessage};
use tinyinference::model::ModelRequest;

// ── ImageAwareMessageTrimMiddleware ───────────────────────────────────────────

/// Flat token cost charged per image — an inline `[IMAGE:…]` marker or a native
/// [`ContentBlock::Image`] block — instead of counting the base64 payload as
/// text. Restores the legacy `harness/token_budget.rs` semantics (issue #4462):
/// the crate `estimate_tokens` prices text at chars/4, so a single large base64
/// image reads as ~2M tokens and the trim believes the context is massively over
/// budget, evicting the whole transcript (system messages included). Providers
/// bill an image at ≈85–1100 tokens by detail; 1200 is a conservative upper
/// bound that keeps the budget realistic without the base64 payload inflating it.
pub(crate) const IMAGE_MARKER_TOKEN_COST: u64 = 1_200;

/// Inline image-marker prefix produced by the multimodal composer
/// (`agent/multimodal.rs`, `compose_multimodal_message`). Priced at
/// [`IMAGE_MARKER_TOKEN_COST`] rather than by its base64 length.
const IMAGE_MARKER_PREFIX: &str = "[IMAGE:";

/// Minimum reply/output reserve — mirrors the legacy `MIN_OUTPUT_RESERVE_TOKENS`.
const MIN_OUTPUT_RESERVE_TOKENS: u64 = 512;

/// Upper anchor for the reply/output reserve — mirrors the legacy
/// `DEFAULT_OUTPUT_RESERVE_TOKENS`.
const DEFAULT_OUTPUT_RESERVE_TOKENS: u64 = 8_192;

/// Rough token estimate (~4 characters per token) with inline `[IMAGE:…]`
/// markers charged a flat [`IMAGE_MARKER_TOKEN_COST`] instead of their base64
/// length. Mirrors the deleted `token_budget::estimate_tokens` (issue #4462).
/// Markerless text takes the fast char/4 path.
pub(crate) fn estimate_text_tokens(text: &str) -> u64 {
    if !text.contains(IMAGE_MARKER_PREFIX) {
        return (text.len() as u64).saturating_add(3) / 4;
    }
    let mut text_bytes: u64 = 0;
    let mut images: u64 = 0;
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(IMAGE_MARKER_PREFIX) {
        let start = cursor + rel;
        text_bytes = text_bytes.saturating_add((start - cursor) as u64); // preceding text
        let after = start + IMAGE_MARKER_PREFIX.len();
        match text[after..].find(']') {
            Some(rel_end) => {
                images += 1;
                cursor = after + rel_end + 1; // skip the whole marker payload
            }
            None => {
                // Unterminated marker — count the remainder as text and stop.
                text_bytes = text_bytes.saturating_add((text.len() - start) as u64);
                cursor = text.len();
                break;
            }
        }
    }
    text_bytes = text_bytes.saturating_add((text.len() - cursor) as u64); // trailing text
    (text_bytes.saturating_add(3) / 4)
        .saturating_add(images.saturating_mul(IMAGE_MARKER_TOKEN_COST))
}

/// Count native [`ContentBlock::Image`] blocks on a message. `Message::text()`
/// concatenates only text blocks, so a native multimodal image would otherwise
/// contribute zero tokens; we charge each one [`IMAGE_MARKER_TOKEN_COST`].
fn count_native_image_blocks(msg: &TaMessage) -> u64 {
    let content = match msg {
        TaMessage::System(m) => &m.content,
        TaMessage::User(m) => &m.content,
        TaMessage::Assistant(m) => &m.content,
        TaMessage::Tool(m) => &m.content,
    };
    content
        .iter()
        .filter(|b| matches!(b, ContentBlock::Image(_)))
        .count() as u64
}

/// Estimate the tokens of a crate [`TaMessage`]: image-aware text tokens, a flat
/// [`IMAGE_MARKER_TOKEN_COST`] per native image block, and the assistant's
/// tool-call name/arguments (which `Message::text()` drops). Mirrors the legacy
/// `estimate_conversation_message_tokens` (issue #4462).
pub(crate) fn estimate_message_tokens(msg: &TaMessage) -> u64 {
    let mut total = estimate_text_tokens(&msg.text());
    total = total
        .saturating_add(count_native_image_blocks(msg).saturating_mul(IMAGE_MARKER_TOKEN_COST));
    if let TaMessage::Assistant(m) = msg {
        for call in &m.tool_calls {
            total = total.saturating_add(estimate_text_tokens(&call.name));
            total = total.saturating_add(estimate_text_tokens(&call.arguments.to_string()));
        }
    }
    total
}

/// Reply/output reserve, mirroring the legacy proportional clamp
/// `clamp(window/10, ≥512, ≤max(8192, window/4))`. Restores the small-window
/// budget the fixed `window − AGENT_TURN_MAX_OUTPUT_TOKENS` regressed (issue
/// #4462): an 8k model reserves ~819 tokens (input budget ~7373), not
/// 16384 → floored 1024.
fn legacy_output_reserve_tokens(window: u64) -> u64 {
    let pct = window / 10;
    pct.max(MIN_OUTPUT_RESERVE_TOKENS)
        .min(DEFAULT_OUTPUT_RESERVE_TOKENS.max(window / 4))
}

/// Input-prompt token budget after reserving room for the reply. Public to the
/// seam so the install site (and tests) can assert the legacy proportional
/// formula (issue #4462).
pub(crate) fn legacy_max_input_tokens(window: u64) -> u64 {
    window.saturating_sub(legacy_output_reserve_tokens(window))
}

/// Deterministic history trim that replaces the crate `MessageTrimMiddleware`
/// (issue #4462), restoring three regression guards the crate trim lost:
///
/// 1. **Image-aware token estimate** — inline `[IMAGE:…]` markers and native
///    image blocks are each charged a flat [`IMAGE_MARKER_TOKEN_COST`] instead
///    of their base64 length, so one large image can no longer read as ~2M
///    tokens and evict the whole transcript.
/// 2. **System messages never dropped** — only non-system history is evictable;
///    the crate trim reorders system messages to the front and drops them as a
///    last resort.
/// 3. **Order preserved + observable** — retained messages keep their original
///    relative order, leading orphaned tool results are snapped past (so no
///    provider 400), and any eviction logs a grep-able `warn` carrying
///    (messages dropped, messages/tokens before-and-after).
pub(crate) struct ImageAwareMessageTrimMiddleware {
    /// Input-prompt token budget (already net of the proportional reply reserve).
    budget: u64,
}

impl ImageAwareMessageTrimMiddleware {
    /// Build a trim middleware whose budget is the legacy proportional
    /// [`legacy_max_input_tokens`] for `window` (issue #4462) — NOT the crate's
    /// fixed `window − 16384`. Floored at 1 so the budget is always positive.
    pub(crate) fn for_context_window(window: u64) -> Self {
        Self {
            budget: legacy_max_input_tokens(window).max(1),
        }
    }
}

#[async_trait]
impl Middleware<()> for ImageAwareMessageTrimMiddleware {
    fn name(&self) -> &str {
        "image_aware_message_trim"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let messages = &mut request.messages;
        let original_tokens: u64 = messages.iter().map(estimate_message_tokens).sum();
        if original_tokens <= self.budget {
            return Ok(());
        }
        let original_len = messages.len();

        // Evict oldest non-system messages first, preserving the relative order
        // of every retained message (rebuilding as `system ++ other` would
        // reorder history when a system message appears after non-system ones —
        // exactly the crate-trim regression). System messages are NEVER dropped.
        let mut removable_positions: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter_map(|(idx, m)| (!matches!(m, TaMessage::System(_))).then_some(idx))
            .collect();

        let mut removed = 0usize;
        while !removable_positions.is_empty() {
            let total: u64 = messages.iter().map(estimate_message_tokens).sum();
            if total <= self.budget {
                break;
            }
            let absolute_idx = removable_positions.remove(0);
            // Subsequent positions shift left by one for every prior removal.
            let remove_at = absolute_idx - removed;
            messages.remove(remove_at);
            removed += 1;
        }

        // Snap the window forward past any leading orphaned tool results: dropping
        // an `assistant(tool_calls)` while keeping its `tool` answer leaves the
        // transcript opening on a tool message with no preceding tool-call, which
        // native providers reject with a 400. Drop leading tool results until the
        // first non-system message is a clean turn boundary.
        while let Some(first_non_system) = messages
            .iter()
            .position(|m| !matches!(m, TaMessage::System(_)))
        {
            if matches!(messages[first_non_system], TaMessage::Tool(_)) {
                messages.remove(first_non_system);
                removed += 1;
            } else {
                break;
            }
        }

        if removed > 0 {
            let final_tokens: u64 = messages.iter().map(estimate_message_tokens).sum();
            tracing::warn!(
                messages_dropped = removed,
                messages_before = original_len,
                messages_after = messages.len(),
                tokens_before = original_tokens,
                tokens_after = final_tokens,
                budget = self.budget,
                "[tinyagents::mw] message_trim evicted oldest history to fit the token budget"
            );
        }
        Ok(())
    }
}
