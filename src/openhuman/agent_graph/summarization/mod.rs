//! Summarization hooks — node-boundary context compression.
//!
//! Thin adapter over the existing harness summarizer
//! ([`crate::openhuman::context::summarize_chat_history`]) so a graph node can
//! compress its working transcript between super-steps without reimplementing
//! the compaction logic. This is the "persistent working state survives
//! compaction" lever from issue #4249: instead of the linear loop's implicit
//! autocompact, a graph can place an explicit `summarize` node on an edge.

use anyhow::Result;

use crate::openhuman::context::{summarize_chat_history, SummaryStats};
use crate::openhuman::inference::provider::{ChatMessage, Provider};

/// Default messages kept verbatim at the tail when a graph node summarizes.
/// Matches the harness default so behavior is consistent with the linear loop.
pub const DEFAULT_KEEP_RECENT: usize = 10;
/// Default summarizer temperature (low, for stable summaries).
pub const DEFAULT_TEMPERATURE: f64 = 0.2;

/// Summarize `history` in place, collapsing older turns into a checkpoint
/// summary while preserving `keep_recent` recent messages and any leading
/// system prompt. Returns what was freed.
pub async fn summarize_node_history(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    model: &str,
    keep_recent: usize,
    temperature: f64,
) -> Result<SummaryStats> {
    tracing::debug!(
        model,
        keep_recent,
        history_len = history.len(),
        "[agent_graph] node-boundary summarization"
    );
    summarize_chat_history(provider, history, model, keep_recent, temperature).await
}
