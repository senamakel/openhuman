//! `tinyagents` integration — drive an openhuman agent turn on the published
//! [`tinyagents`](https://crates.io/crates/tinyagents) orchestration framework
//! (issue #4249).
//!
//! openhuman's agent execution runs on the `tinyagents` crate
//! (LangGraph/LangChain-style durable graphs + an agent-loop harness with model/
//! tool registries, middleware, retry/fallback, and limits). This module is the
//! **adapter seam**: it bridges openhuman's `Provider`, `Tool`, and `ChatMessage`
//! types onto the crate's `ChatModel`, `Tool`, and `Message` traits, then drives
//! a turn through [`AgentHarness::invoke`]. The chat / channel / sub-agent
//! routes call [`run_turn_via_tinyagents_shared`] (default ON in production).
//!
//! The chat route is at functional parity with the legacy `run_turn_engine`:
//! the [`OpenhumanEventBridge`] mirrors the 0.2.0 harness event stream onto
//! `AgentProgress` (live tool timeline, incremental text deltas, cost footer),
//! [`ProviderModel::stream`] forwards true token streaming, multimodal markers
//! are expanded, and history is trimmed to the context window. Remaining gaps:
//! per-iteration steering and the `ask_user_clarification` early-exit pause.

mod convert;
mod model;
pub mod observability;
mod tools;

use std::sync::Arc;

use anyhow::Result;
use tinyagents::harness::context::{RunConfig, RunContext};
use tinyagents::harness::events::EventSink;
use tinyagents::harness::message::Message as TaMessage;
use tinyagents::harness::middleware::MessageTrimMiddleware;
use tinyagents::harness::runtime::AgentHarness;
use tinyagents::harness::steering::{SteeringCommand, SteeringHandle};
use tinyagents::harness::summarization::TrimStrategy;

use crate::openhuman::agent::harness::run_queue::RunQueue;
use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::inference::provider::{ChatMessage, Provider};

pub use model::ProviderModel;
pub use observability::OpenhumanEventBridge;
pub use tools::{SharedToolAdapter, ToolAdapter};

use std::collections::HashSet;
use tokio::sync::mpsc::Sender;

/// Whether agent turns (chat) should route through the `tinyagents` harness.
///
/// **Default ON in production** (issue #4249): the tinyagents harness is the
/// agent engine. Set `OPENHUMAN_AGENT_GRAPH_TINYAGENTS=0` to fall back to the
/// legacy `run_turn_engine` (e.g. when the missing streaming/cost/multimodal
/// seams matter). Under `cfg(test)` the default is OFF so the legacy-engine
/// unit tests keep validating the fallback path; an explicit env value always
/// wins in either build.
pub fn tinyagents_routing_enabled() -> bool {
    routing_default_with_override("OPENHUMAN_AGENT_GRAPH_TINYAGENTS")
}

/// Shared default-on-in-prod / off-under-test resolution for the route flags.
/// An explicit `0`/`false` or `1`/`true` env value always overrides.
pub(crate) fn routing_default_with_override(var: &str) -> bool {
    match std::env::var(var) {
        Ok(v) => !(v == "0" || v.eq_ignore_ascii_case("false")),
        Err(_) => !cfg!(test),
    }
}

/// Drain the run queue's pending steer messages and forward them to the
/// tinyagents [`SteeringHandle`] as injected user turns (the harness applies
/// them to the working transcript at the next iteration checkpoint). This is the
/// bridge behind the `steer_subagent` / mid-flight-steering feature.
async fn forward_steers(queue: &RunQueue, handle: &SteeringHandle) {
    for msg in queue.drain_steers().await {
        handle.send(SteeringCommand::InjectMessage(TaMessage::user(format!(
            "[User steering message]: {}",
            msg.text
        ))));
    }
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
        "[tinyagents] routing agent turn through tinyagents harness"
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

/// Drive a turn through the tinyagents harness over the routes' **shared**,
/// `Arc`-owned tool registry sets (`Arc<Vec<Box<dyn Tool>>>`), advertising
/// exactly `specs` (already filtered/deduped by the caller's visibility rules).
///
/// This is the entry point the channel/sub-agent routes use to retire the
/// in-house `live` turn machine: it registers a [`SharedToolAdapter`] per
/// advertised spec so the same `Arc`-shared tools the legacy loop runs are
/// reused without cloning.
///
/// `allowed` is the callable tool-name whitelist (empty = every tool visible in
/// `tool_sets`); each callable tool is advertised via its own `spec()`.
///
/// When `on_progress` is `Some`, the run streams (`invoke_streaming_in_context`)
/// and a [`OpenhumanEventBridge`] mirrors the harness event stream onto
/// `AgentProgress` (live tool timeline, text deltas, cost/token footer) and the
/// global cost tracker — restoring the seams the legacy `run_turn_engine`
/// produced. Pass `None` for fire-and-forget turns (channel/sub-agent) that
/// only need the final text.
///
/// When `context_window` is known, a [`MessageTrimMiddleware`] keeps history
/// under budget (autocompaction parity).
///
/// Parity note (issue #4249): per-iteration steering and the
/// `ask_user_clarification` early-exit pause are not yet wired on this path.
/// (Incremental streaming, the live tool timeline, the cost footer, and
/// multimodal expansion are bridged on the chat route.)
#[allow(clippy::too_many_arguments)]
pub async fn run_turn_via_tinyagents_shared(
    provider: Arc<dyn Provider>,
    model: &str,
    temperature: f64,
    history: Vec<ChatMessage>,
    tool_sets: Vec<Arc<Vec<Box<dyn crate::openhuman::tools::Tool>>>>,
    allowed: HashSet<String>,
    max_iterations: usize,
    on_progress: Option<Sender<AgentProgress>>,
    context_window: Option<u64>,
    run_queue: Option<Arc<RunQueue>>,
) -> Result<TinyagentsTurnOutcome> {
    let mut harness: AgentHarness<()> = AgentHarness::new();
    harness
        .register_model(
            model,
            Arc::new(ProviderModel::new(provider, model, temperature)),
        )
        .set_default_model(model);

    // Autocompaction parity: when the provider's context window is known, trim
    // history from the front (system messages kept) so a long thread stays under
    // budget before each model call — the deterministic, no-extra-LLM-call
    // analogue of the legacy `ContextManager` reduce-before-call.
    if let Some(window) = context_window.filter(|w| *w > 0) {
        let budget = window.saturating_sub(
            crate::openhuman::inference::provider::AGENT_TURN_MAX_OUTPUT_TOKENS as u64,
        );
        harness.push_middleware(Arc::new(MessageTrimMiddleware::new(
            TrimStrategy::MaxTokens(budget.max(1024)),
        )));
    }

    // Register one adapter per unique callable tool name found across the shared
    // sets (newest set wins on a name clash; `allowed` empty = all visible).
    let mut registered: HashSet<String> = HashSet::new();
    for name in tool_sets
        .iter()
        .flat_map(|set| set.iter())
        .map(|t| t.name())
    {
        if !registered.contains(name) && (allowed.is_empty() || allowed.contains(name)) {
            if let Some(adapter) = SharedToolAdapter::for_name(tool_sets.clone(), name) {
                registered.insert(name.to_string());
                harness.register_tool(Arc::new(adapter));
            }
        }
    }
    let tool_count = registered.len();

    let config = RunConfig::new("agent_turn")
        .with_max_model_calls(max_iterations)
        .with_max_tool_calls(max_iterations.saturating_mul(8).max(8));

    tracing::info!(
        model,
        max_iterations,
        tools = tool_count,
        observed = on_progress.is_some(),
        "[tinyagents] routing turn through tinyagents harness (shared tools)"
    );

    let input = convert::history_to_messages(&history);

    // Build the run context: optional progress/cost bridge (streaming) + optional
    // mid-flight steering from the run queue.
    let mut ctx = RunContext::new(config, ());

    let streaming = on_progress.is_some();
    let bridge = on_progress.map(|tx| {
        let bridge = OpenhumanEventBridge::new(Some(tx), model, max_iterations);
        let events = EventSink::new();
        events.subscribe(bridge.clone());
        (bridge, events)
    });
    if let Some((_, events)) = &bridge {
        ctx = ctx.with_events(events.clone());
    }

    // Steering: drain any already-queued steer messages into the handle (so a
    // pre-run steer lands before the first model call), attach it, and forward
    // mid-flight steers via a poller aborted when the run returns.
    let steering_forwarder = if let Some(queue) = run_queue {
        let handle = SteeringHandle::allow_all();
        forward_steers(&queue, &handle).await;
        ctx = ctx.with_steering(handle.clone());
        let q = queue.clone();
        Some(tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                forward_steers(&q, &handle).await;
            }
        }))
    } else {
        None
    };

    let run_result = if streaming {
        harness.invoke_streaming_in_context(&(), ctx, input).await
    } else {
        harness.invoke_in_context(&(), ctx, input).await
    };
    if let Some(forwarder) = steering_forwarder {
        forwarder.abort();
    }
    let run = run_result.map_err(|e| anyhow::anyhow!("tinyagents harness run failed: {e}"))?;
    let bridge_totals = bridge.map(|(bridge, _)| {
        let (input_tokens, output_tokens, _) = bridge.totals();
        (input_tokens, output_tokens)
    });

    // Prefer the bridge's accumulated usage (per-call, authoritative) when the
    // observed path ran; otherwise fall back to the run's aggregate totals.
    let (input_tokens, output_tokens) =
        bridge_totals.unwrap_or((run.usage.usage.input_tokens, run.usage.usage.output_tokens));

    Ok(TinyagentsTurnOutcome {
        text: run.text().unwrap_or_default(),
        history: convert::messages_to_history(&run.messages),
        model_calls: run.model_calls,
        tool_calls: run.tool_calls,
        input_tokens,
        output_tokens,
    })
}

#[cfg(test)]
mod tests;
