//! Pure helpers that shape a generation span's attributes: the model label,
//! usage normalization across provider routes, cost provenance, and the
//! unstreamed-call guard for time to first token.

use crate::agent::cost;
use crate::inference::provider::BilledUsage;

/// A call whose first delta landed this close to its completion was not
/// streamed: non-streaming providers (the OpenAI Responses path used by the
/// ChatGPT/Codex sign-in) emit the whole reply as one synthetic delta right
/// before completing, so its "first token" is just the completion instant.
pub(super) const UNSTREAMED_DELTA_WINDOW_MS: u64 = 100;

/// Model name to report for a call: the event's own, else the scope's last
/// known model, else `"unknown"`. A child bridge or a journal completion whose
/// `ModelStarted` was lost reports an empty name, which surfaced in Langfuse
/// as `llm.` / model `.`.
pub(super) fn resolve_model(model: &str, fallback: Option<&str>) -> String {
    let model = model.trim();
    if !model.is_empty() {
        return model.to_string();
    }
    fallback
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

/// Langfuse-facing model label: `{provider}.{model}`, or the bare model when
/// the provider is unknown (the journal projection has none), instead of a
/// dangling `.{model}`.
pub(super) fn model_label(provider_id: &str, model: &str) -> String {
    let provider = provider_id.trim();
    if provider.is_empty() {
        model.to_string()
    } else {
        format!("{provider}.{model}")
    }
}

/// One call's token usage in a single convention, whatever the route reported.
///
/// Routes disagree on what `input_tokens` means: most (OpenAI-compatible,
/// OpenRouter, the Anthropic API adapter) report the whole prompt, cache
/// reads and writes included; the Claude Code route forwards Anthropic's raw
/// `input_tokens`, which counts only the uncached remainder. Exported as-is,
/// the second made Langfuse's `total` smaller than the cache read alone. A
/// prompt-inclusive count can never be below `cache_read + cache_creation`,
/// so a count that is must be the uncached remainder and is widened here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NormalizedUsage {
    /// Whole prompt: uncached + cache read + cache creation.
    pub(super) input_total: u64,
    /// Prompt tokens neither read from nor written to a cache.
    pub(super) input_uncached: u64,
    pub(super) cache_read: u64,
    pub(super) cache_creation: u64,
    pub(super) output: u64,
    /// `input_total + output`.
    pub(super) total: u64,
    /// Whether the route reported the uncached remainder and it was widened.
    pub(super) widened: bool,
}

pub(super) fn normalize_usage(
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    cache_creation: u64,
) -> NormalizedUsage {
    let cached = cache_read.saturating_add(cache_creation);
    let widened = input_tokens < cached;
    let input_total = if widened {
        input_tokens.saturating_add(cached)
    } else {
        input_tokens
    };
    NormalizedUsage {
        input_total,
        input_uncached: input_total.saturating_sub(cached),
        cache_read,
        cache_creation,
        output: output_tokens,
        total: input_total.saturating_add(output_tokens),
        widened,
    }
}

/// Where a generation's cost figure came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CostSource {
    /// A provider charge or an estimate from a known price.
    Priced,
    /// The model has no known price and the reported figure was the
    /// estimator's placeholder rate; no cost is recorded.
    Unpriced,
}

impl CostSource {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Priced => "priced",
            Self::Unpriced => "unpriced",
        }
    }
}

/// The cost to record for a call, and its provenance.
///
/// The live bridge prices a call with [`cost::estimate_call_cost_usd`] when
/// the provider reports no charge, and that estimator falls back to a
/// placeholder rate for an unknown model (kept there so budget caps still
/// bite). On a trace that placeholder reads as a real charge, so a figure
/// that is exactly the placeholder estimate for an unpriced model is dropped:
/// the generation carries no cost and Langfuse falls back to its own model
/// price table, if it has one. A provider charge differs from that estimate
/// and is kept.
pub(super) fn effective_cost(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    reported_usd: f64,
) -> (f64, CostSource) {
    if cost::lookup_known_pricing(model).is_some() {
        return (reported_usd, CostSource::Priced);
    }
    let placeholder = cost::estimate_call_cost_usd(
        model,
        &BilledUsage::from_counts(input_tokens, output_tokens).with_cached_input_tokens(cache_read),
    );
    if (reported_usd - placeholder).abs() <= 1e-12 {
        (0.0, CostSource::Unpriced)
    } else {
        (reported_usd, CostSource::Priced)
    }
}

/// Whether a call with these delta stamps was really streamed. A call is
/// treated as unstreamed when its first delta arrived within
/// [`UNSTREAMED_DELTA_WINDOW_MS`] of completion but well after it started:
/// that delta carries the completion instant, not a first token.
pub(super) fn is_unstreamed(first_delta_ms: u64, start_ms: u64, end_ms: u64) -> bool {
    end_ms.saturating_sub(first_delta_ms) <= UNSTREAMED_DELTA_WINDOW_MS
        && first_delta_ms.saturating_sub(start_ms) > UNSTREAMED_DELTA_WINDOW_MS
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
