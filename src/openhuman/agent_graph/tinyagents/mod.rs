//! `tinyagents` integration — drive an openhuman agent turn on the published
//! [`tinyagents`](https://crates.io/crates/tinyagents) orchestration framework
//! (issue #4249).
//!
//! The `agent_graph` domain is migrating off its in-house engine onto the
//! `tinyagents` crate (LangGraph/LangChain-style durable graphs + an agent-loop
//! harness with model/tool registries, middleware, retry/fallback, and limits).
//! This module is the **adapter seam**: it bridges openhuman's `Provider`,
//! `Tool`, and `ChatMessage` types onto the crate's `ChatModel`, `Tool`, and
//! `Message` traits, then drives a turn through [`AgentHarness::invoke`].
//!
//! Gated by `OPENHUMAN_AGENT_GRAPH_TINYAGENTS` and **off by default**, mirroring
//! the existing `live`/`channel`/`subagent` routes. It is an explicit subset
//! today — the harness owns its own native tool-call threading and transcript,
//! so per-iteration steering, the payload summarizer, and multimodal prep are
//! not yet wired on this path. It is opt-in until those land.

mod convert;
mod model;
mod tools;

use std::sync::Arc;

use anyhow::Result;
use tinyagents::harness::context::RunConfig;
use tinyagents::harness::runtime::AgentHarness;

use crate::openhuman::inference::provider::{ChatMessage, Provider};

pub use model::ProviderModel;
pub use tools::ToolAdapter;

/// Whether agent turns should route through the `tinyagents` harness.
pub fn tinyagents_routing_enabled() -> bool {
    std::env::var("OPENHUMAN_AGENT_GRAPH_TINYAGENTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// The outcome of a turn driven on the `tinyagents` harness.
#[derive(Debug, Clone)]
pub struct TinyagentsTurnOutcome {
    /// Final assistant text.
    pub text: String,
    /// The full transcript, converted back to openhuman messages.
    pub history: Vec<ChatMessage>,
    /// Number of model calls the loop made.
    pub model_calls: usize,
    /// Number of tool calls the loop made.
    pub tool_calls: usize,
    /// Accumulated input tokens.
    pub input_tokens: u64,
    /// Accumulated output tokens.
    pub output_tokens: u64,
}

/// Drive an agent turn through the `tinyagents` agent-loop harness.
///
/// Registers `provider` as the default model and every entry in `resolved_tools`
/// as a harness tool, seeds the loop with `history`, and runs the loop bounded
/// by `max_iterations` model calls. Returns the final text plus the resulting
/// transcript translated back to openhuman [`ChatMessage`]s.
pub async fn run_turn_via_tinyagents(
    provider: Arc<dyn Provider>,
    model: &str,
    temperature: f64,
    history: Vec<ChatMessage>,
    resolved_tools: Vec<Arc<dyn crate::openhuman::tools::Tool>>,
    max_iterations: usize,
) -> Result<TinyagentsTurnOutcome> {
    let mut harness: AgentHarness<()> = AgentHarness::new();
    harness
        .register_model(
            model,
            Arc::new(ProviderModel::new(provider, model, temperature)),
        )
        .set_default_model(model);
    let tool_count = resolved_tools.len();
    for tool in resolved_tools {
        harness.register_tool(Arc::new(ToolAdapter::new(tool)));
    }

    // Bound the run: one model call per legacy "iteration", and allow generous
    // tool calls (the loop also stops when the model stops requesting tools).
    let config = RunConfig::new("agent_turn")
        .with_max_model_calls(max_iterations)
        .with_max_tool_calls(max_iterations.saturating_mul(8).max(8));

    tracing::info!(
        model,
        max_iterations,
        tools = tool_count,
        "[agent_graph::tinyagents] routing agent turn through tinyagents harness"
    );

    let input = convert::history_to_messages(&history);
    let run = harness
        .invoke(&(), (), config, input)
        .await
        .map_err(|e| anyhow::anyhow!("tinyagents harness run failed: {e}"))?;

    let text = run.text().unwrap_or_default();
    let out_history = convert::messages_to_history(&run.messages);

    Ok(TinyagentsTurnOutcome {
        text,
        history: out_history,
        model_calls: run.model_calls,
        tool_calls: run.tool_calls,
        input_tokens: run.usage.usage.input_tokens,
        output_tokens: run.usage.usage.output_tokens,
    })
}

#[cfg(test)]
mod tests;
