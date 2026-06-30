//! LLM-backed conversation summarization for the tinyagents harness path
//! (issue #4249).
//!
//! The harness historically only **front-trimmed** a long transcript
//! ([`tinyagents::harness::middleware::MessageTrimMiddleware`] with
//! [`TrimStrategy::MaxTokens`][tinyagents::harness::summarization::TrimStrategy]),
//! dropping the oldest turns wholesale once the thread neared the model's
//! context window. That is lossy: the dropped turns vanish.
//!
//! This module supplies the missing **summarization step** the per-folder graphs
//! install: an LLM-backed [`Summarizer`] that condenses the older slice of the
//! transcript into a single system message, driven by the crate's
//! [`ContextCompressionMiddleware`][tinyagents::harness::middleware::ContextCompressionMiddleware]
//! and a context-window-aware [`SummarizationPolicy`]. The policy only fires once
//! the running token estimate crosses [`SUMMARIZE_THRESHOLD_FRACTION`] of the
//! **current model's** context window — so the trigger is keyed to "whatever
//! model we are using", exactly mirroring the existing
//! [`ContextGuard`][crate::openhuman::context] compaction threshold (0.90).
//!
//! Layering: a graph installs the compression middleware **before** the
//! deterministic trim, so summarization is preferred and trimming remains only a
//! last-resort hard cap when even the summary + recent window overflow.

use std::sync::Arc;

use async_trait::async_trait;

use tinyagents::error::{Result as TaResult, TinyAgentsError};
use tinyagents::harness::message::Message as TaMessage;
use tinyagents::harness::summarization::{
    estimate_tokens, CompressionProvenance, SummarizationPolicy, Summarizer, SummaryRecord,
};

use crate::openhuman::context::summarizer::SUMMARIZER_SYSTEM_PROMPT;
use crate::openhuman::inference::provider::Provider;

/// Fraction of the model's context window at which summarization fires.
///
/// Mirrors `ContextGuard::COMPACTION_TRIGGER_THRESHOLD` (0.90) so the tinyagents
/// path compacts at the same point the legacy `ContextManager` did.
pub const SUMMARIZE_THRESHOLD_FRACTION: f64 = 0.90;

/// Number of most-recent non-system messages kept verbatim after a compaction.
/// The older head is folded into the summary; this tail stays untouched so the
/// model retains the live working context.
pub const SUMMARIZE_KEEP_LAST: usize = 8;

/// An LLM-backed [`Summarizer`] that condenses a slice of harness [`TaMessage`]s
/// into a single system summary via an openhuman [`Provider`] chat call.
///
/// Wraps the **same** provider + model the turn is already running on, so the
/// summary is produced by the active model (a cheaper summarizer model can be
/// threaded later if compaction on the main model proves expensive — the legacy
/// `ContextConfig::summarizer_model` hook).
pub struct ProviderModelSummarizer {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
}

impl ProviderModelSummarizer {
    /// Build a summarizer over `provider`/`model` at `temperature`.
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>, temperature: f64) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
        }
    }
}

/// Role label for a harness message, used to render the plain-text transcript
/// the summarizer reads.
fn role_label(msg: &TaMessage) -> &'static str {
    match msg {
        TaMessage::System(_) => "system",
        TaMessage::User(_) => "user",
        TaMessage::Assistant(_) => "assistant",
        TaMessage::Tool(_) => "tool",
    }
}

#[async_trait]
impl Summarizer for ProviderModelSummarizer {
    async fn summarize(&self, messages: &[TaMessage]) -> TaResult<SummaryRecord> {
        if messages.is_empty() {
            return Err(TinyAgentsError::Validation(
                "cannot summarize an empty message list".into(),
            ));
        }

        let original_token_estimate: u64 =
            messages.iter().map(|m| estimate_tokens(&m.text())).sum();
        // `Message` carries no stable id, so assign synthetic positional ids
        // (matching the crate's `ConcatSummarizer` provenance convention).
        let source_ids: Vec<String> = (0..messages.len()).map(|i| format!("msg-{i}")).collect();

        let transcript = messages
            .iter()
            .map(|m| format!("{}: {}", role_label(m), m.text()))
            .collect::<Vec<_>>()
            .join("\n");

        tracing::info!(
            model = %self.model,
            head_messages = messages.len(),
            approx_input_tokens = original_token_estimate,
            "[tinyagents::summarize] dispatching context-window summary"
        );

        let summary = self
            .provider
            .chat_with_system(
                Some(SUMMARIZER_SYSTEM_PROMPT),
                &transcript,
                &self.model,
                self.temperature,
            )
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "[tinyagents::summarize] summarizer provider call failed");
                TinyAgentsError::Model(format!("summarizer provider call failed: {e}"))
            })?;

        let summary = summary.trim();
        if summary.is_empty() {
            return Err(TinyAgentsError::Model(
                "summarizer returned empty response".into(),
            ));
        }

        let body = format!("=== Conversation Summary (compacted) ===\n{summary}");
        let summary_token_estimate = estimate_tokens(&body);

        tracing::info!(
            model = %self.model,
            summary_tokens = summary_token_estimate,
            freed_tokens = original_token_estimate.saturating_sub(summary_token_estimate),
            "[tinyagents::summarize] context-window summary complete"
        );

        Ok(SummaryRecord {
            summary: TaMessage::system(body),
            provenance: CompressionProvenance {
                source_ids,
                original_token_estimate,
                summary_token_estimate,
                reason: format!(
                    "ProviderModelSummarizer via {} (LLM compaction at {:.0}% of context window)",
                    self.model,
                    SUMMARIZE_THRESHOLD_FRACTION * 100.0
                ),
            },
        })
    }
}

/// Build the context-window-aware [`SummarizationPolicy`] for a model whose
/// input window is `context_window` tokens.
///
/// The policy triggers compaction once the estimated transcript tokens reach
/// `context_window * `[`SUMMARIZE_THRESHOLD_FRACTION`] and keeps the most recent
/// [`SUMMARIZE_KEEP_LAST`] non-system messages (plus all system messages)
/// verbatim. Pair it with [`ProviderModelSummarizer`] via
/// [`ContextCompressionMiddleware::with_summarizer`][tinyagents::harness::middleware::ContextCompressionMiddleware::with_summarizer].
pub fn summarization_policy(context_window: u64) -> SummarizationPolicy {
    let mut policy = SummarizationPolicy::default()
        .with_context_window(context_window)
        .with_threshold_fraction(SUMMARIZE_THRESHOLD_FRACTION);
    policy.keep_last = SUMMARIZE_KEEP_LAST;
    policy
}
