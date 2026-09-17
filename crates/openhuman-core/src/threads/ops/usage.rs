//! Aggregated token/cost usage for a thread, re-audited at current pricing.

use super::support::{counts, envelope, workspace_dir};
use crate::memory::ApiEnvelope;
use crate::rpc::RpcOutcome;

/// Request for [`token_usage`]: the thread whose persisted usage to total.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ThreadTokenUsageRequest {
    pub thread_id: String,
}

/// Aggregated token/cost usage for one thread, read back from its persisted
/// session transcripts. Seeds the UI footer when the user selects a thread so
/// the totals reflect prior turns instead of starting at zero.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThreadTokenUsageResponse {
    pub thread_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cost_usd: f64,
    pub turn_count: usize,
    /// Tokens of the most recent turn — numerator for the context-window gauge.
    pub last_turn_input_tokens: u64,
    pub last_turn_output_tokens: u64,
    /// Context window (tokens) inferred from the last model; `0` when unknown.
    pub context_window: u64,
    pub model: Option<String>,
    pub updated: Option<String>,
    /// `false` when the thread has no persisted turns yet (all zeros).
    pub has_usage: bool,
    /// Per-archetype sub-agent spend (re-audited at current pricing). The
    /// top-level totals already include this; it's broken out for the UI's
    /// per-agent footer rows.
    pub subagents: Vec<SubagentUsageDto>,
}

/// One sub-agent archetype's contribution within a thread.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubagentUsageDto {
    pub agent_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub runs: usize,
}

/// Total a thread's persisted token/cost usage across its root transcripts.
pub async fn token_usage(
    request: ThreadTokenUsageRequest,
) -> Result<RpcOutcome<ApiEnvelope<ThreadTokenUsageResponse>>, String> {
    let dir = workspace_dir().await?;
    let summary = crate::agent::harness::session::transcript::read_thread_usage_summary(
        &dir,
        &request.thread_id,
    );

    // Re-audit cost at CURRENT pricing rather than trusting the
    // `charged_amount_usd` persisted in the transcript: those values were
    // stamped at turn time and don't reflect later tier-pricing corrections.
    // Recompute from the persisted token counts using the last-known model's
    // rates; falls back to `fallback` only when the model is unknown.
    let audit_cost =
        |model: Option<&str>, input: u64, output: u64, cached: u64, fallback: f64| match model {
            Some(m) => crate::agent::cost::estimate_call_cost_usd(
                m,
                &crate::inference::provider::UsageInfo {
                    input_tokens: input,
                    output_tokens: output,
                    cached_input_tokens: cached,
                    ..Default::default()
                },
            ),
            None => fallback,
        };

    let response = match summary {
        Some(s) => {
            let context_window = s
                .model
                .as_deref()
                .and_then(crate::inference::model_context::context_window_for_model)
                .unwrap_or(0);

            // Orchestrator (root) spend, re-audited.
            let orchestrator_cost = audit_cost(
                s.model.as_deref(),
                s.input_tokens,
                s.output_tokens,
                s.cached_input_tokens,
                s.cost_usd,
            );

            // Sub-agent archetypes, each re-audited with its own model. Older
            // sub-agent transcripts didn't persist a model on their messages, so
            // fall back to the thread's (root) model rather than pricing them at
            // $0 — sub-agents usually run on the same managed tier as the parent.
            let mut subagents = Vec::with_capacity(s.subagents.len());
            let (mut sub_in, mut sub_out, mut sub_cached, mut sub_cost) = (0u64, 0u64, 0u64, 0.0);
            for g in &s.subagents {
                let sub_model = g.model.as_deref().or(s.model.as_deref());
                let cost = audit_cost(
                    sub_model,
                    g.input_tokens,
                    g.output_tokens,
                    g.cached_input_tokens,
                    0.0,
                );
                sub_in = sub_in.saturating_add(g.input_tokens);
                sub_out = sub_out.saturating_add(g.output_tokens);
                sub_cached = sub_cached.saturating_add(g.cached_input_tokens);
                sub_cost += cost;
                subagents.push(SubagentUsageDto {
                    agent_id: g.agent_id.clone(),
                    input_tokens: g.input_tokens,
                    output_tokens: g.output_tokens,
                    cost_usd: cost,
                    runs: g.runs,
                });
            }

            // Top-level totals = orchestrator + all sub-agents.
            ThreadTokenUsageResponse {
                thread_id: request.thread_id.clone(),
                input_tokens: s.input_tokens.saturating_add(sub_in),
                output_tokens: s.output_tokens.saturating_add(sub_out),
                cached_input_tokens: s.cached_input_tokens.saturating_add(sub_cached),
                cost_usd: orchestrator_cost + sub_cost,
                turn_count: s.turn_count,
                last_turn_input_tokens: s.last_turn_input_tokens,
                last_turn_output_tokens: s.last_turn_output_tokens,
                context_window,
                model: s.model,
                updated: Some(s.updated),
                has_usage: true,
                subagents,
            }
        }
        None => ThreadTokenUsageResponse {
            thread_id: request.thread_id.clone(),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            cost_usd: 0.0,
            turn_count: 0,
            last_turn_input_tokens: 0,
            last_turn_output_tokens: 0,
            context_window: 0,
            model: None,
            updated: None,
            has_usage: false,
            subagents: Vec::new(),
        },
    };

    let has_usage = response.has_usage;
    Ok(envelope(
        response,
        Some(counts([("has_usage", usize::from(has_usage))])),
        None,
    ))
}
