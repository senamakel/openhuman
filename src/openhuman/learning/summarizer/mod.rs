//! Summarizer abstraction for the learning subsystem (#1419).
//!
//! Reflection and transcript-ingest fire often and need a model — but they
//! produce short, structured summaries that a cheap model handles fine.
//! Running them on the orchestrator-tier model is wasteful. This module
//! introduces a dedicated [`SummarizerProvider`] trait, a thin
//! [`ConfiguredSummarizer`] implementation that wraps an existing
//! [`crate::openhuman::providers::Provider`], and a tool-call digest layer
//! ([`digest`]) so reflection never feeds raw multi-call tool history to the
//! summarizer's smaller context window.
//!
//! The summarizer is **opt-in**: when no summarizer is configured, the
//! reflection / transcript-ingest paths keep behaving the way they did
//! before #1419 — heuristic-only for transcript ingest, orchestrator-tier
//! provider for reflection. That keeps offline users on the heuristic
//! fast-path from #1406 without silently breaking.

pub mod digest;

use crate::openhuman::providers::Provider;
use async_trait::async_trait;
use std::sync::Arc;

pub use digest::{compress_tool_calls, render_tool_digests, ToolCallDigest};

/// Minimum context-window cap (in characters) we'll honour. Anything
/// smaller would clip even a one-paragraph reflection prompt, so we
/// floor at this value rather than failing silently.
pub const MIN_SUMMARIZER_CONTEXT_CHARS: usize = 1024;

/// Default character cap for the summarizer's context window. Tuned to
/// fit comfortably inside a small local model (e.g. 4k-token Ollama
/// chat models) after prompt scaffolding and reserved completion space.
pub const DEFAULT_SUMMARIZER_CONTEXT_CHARS: usize = 6_000;

/// Trait for a cheap summarizer model, distinct from the orchestrator
/// provider. Implementations promise:
///
/// - **Cheap and fast** — callers fire this in the background on every
///   threshold crossing, so latency / cost matters more than peak
///   quality.
/// - **Bounded context** — [`context_window_chars`] is the *callable*
///   budget for `prompt`. Callers must compress inputs (see [`digest`])
///   to fit.
/// - **Short outputs** — the trait targets sub-paragraph summaries.
///   `prompt` returns the raw model response; parsing is the caller's
///   responsibility.
#[async_trait]
pub trait SummarizerProvider: Send + Sync {
    /// Approximate character budget for `prompt` input. Callers must
    /// truncate / digest inputs to stay under this cap; the
    /// implementation makes no guarantee about behaviour when the cap
    /// is exceeded.
    fn context_window_chars(&self) -> usize;

    /// Human-readable identifier used in logs / telemetry. Should not
    /// include credentials or absolute paths.
    fn label(&self) -> &str;

    /// Run a one-shot summarization. Returns the model's raw response.
    async fn prompt(&self, prompt: &str) -> anyhow::Result<String>;
}

/// Thin [`SummarizerProvider`] backed by an existing
/// [`Provider`]. Used by the cloud path; the local path is wired
/// separately by the caller (see `learning::reflection`).
pub struct ConfiguredSummarizer {
    provider: Arc<dyn Provider>,
    model_hint: String,
    context_window_chars: usize,
    label: String,
}

impl ConfiguredSummarizer {
    /// Construct a configured summarizer.
    ///
    /// `context_window_chars` is clamped to at least
    /// [`MIN_SUMMARIZER_CONTEXT_CHARS`] so a misconfigured value cannot
    /// produce zero-byte truncation downstream.
    pub fn new(
        provider: Arc<dyn Provider>,
        model_hint: impl Into<String>,
        context_window_chars: usize,
        label: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            model_hint: model_hint.into(),
            context_window_chars: context_window_chars.max(MIN_SUMMARIZER_CONTEXT_CHARS),
            label: label.into(),
        }
    }
}

#[async_trait]
impl SummarizerProvider for ConfiguredSummarizer {
    fn context_window_chars(&self) -> usize {
        self.context_window_chars
    }

    fn label(&self) -> &str {
        &self.label
    }

    async fn prompt(&self, prompt: &str) -> anyhow::Result<String> {
        let started = std::time::Instant::now();
        let input_chars = prompt.chars().count();
        let fill_ratio = (input_chars as f32) / (self.context_window_chars.max(1) as f32);
        log::debug!(
            "[summarizer] dispatch label={} model={} input_chars={} cap={} fill={:.2}",
            self.label,
            self.model_hint,
            input_chars,
            self.context_window_chars,
            fill_ratio,
        );
        let result = self
            .provider
            .simple_chat(prompt, &self.model_hint, 0.2)
            .await;
        let elapsed = started.elapsed();
        match &result {
            Ok(out) => log::info!(
                "[summarizer] label={} model={} input_chars={} output_chars={} latency_ms={}",
                self.label,
                self.model_hint,
                input_chars,
                out.chars().count(),
                elapsed.as_millis(),
            ),
            Err(e) => log::warn!(
                "[summarizer] label={} model={} input_chars={} latency_ms={} error={e}",
                self.label,
                self.model_hint,
                input_chars,
                elapsed.as_millis(),
            ),
        }
        result
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
