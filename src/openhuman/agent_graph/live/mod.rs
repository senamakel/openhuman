//! Phase A of the harness migration (issue #4249): make the `agent_graph`
//! engine able to drive a **real** agent turn — a live provider + tool-call loop
//! — not just a synthetic `ProductState` demo.
//!
//! ## The architectural blocker, solved
//!
//! The generic engine takes an owned, `Clone`-able, `Serialize`-able
//! [`GraphState`] and may run nodes concurrently. A live turn instead needs
//! exclusive `&mut` access to non-clonable machinery (the provider, the growing
//! message history, the tool executor). We bridge the two by keeping that
//! machinery in a [`LiveTurnMachine`] behind an `Arc<tokio::sync::Mutex<…>>`
//! that the nodes capture, while the engine-visible [`LiveTurnState`] stays
//! tiny and serializable (just routing bookkeeping). For the canonical turn —
//! a single linear chain with no parallel forks — exactly one node runs at a
//! time, so the mutex is never contended and the borrow rules are satisfied.
//!
//! This is the foundation the later phases build on: once a graph can run a real
//! turn, the entry points (`run_subagent`, `Agent::turn`, the channel loop) can
//! be routed through per-agent blueprints and the legacy `run_turn_engine`
//! retired.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::openhuman::agent::cost::TurnCost;
use crate::openhuman::agent_graph::graph::{GraphState, Node, NodeCtx, NodeOutput, StateGraph};
use crate::openhuman::agent_graph::types::GraphError;
use crate::openhuman::inference::provider::{ChatMessage, ChatRequest, Provider, ToolCall};
use crate::openhuman::tools::ToolSpec;

/// Executes one tool call. The real adapter ([`HarnessToolExecutor`]) wraps the
/// harness tool registry; tests supply a deterministic mock.
#[async_trait]
pub trait LiveToolExecutor: Send + Sync {
    /// Run `name(arguments)` and return `(output, success)`.
    async fn execute(&self, name: &str, arguments: &str) -> (String, bool);
}

/// Real tool executor over the harness [`Tool`] registry (Phase B).
///
/// Holds the resolved, shareable tool set (`Arc<dyn Tool>` so it satisfies the
/// engine's `'static` node bound) and dispatches a call by name, parsing the
/// JSON arguments and rendering the [`ToolResult`] the way the LLM should see
/// it. This is the bridge that lets a graph-driven turn run the *same* tools the
/// legacy loop runs.
pub struct HarnessToolExecutor {
    tools: Vec<Arc<dyn crate::openhuman::tools::Tool>>,
}

impl HarnessToolExecutor {
    /// Build from a resolved tool set.
    pub fn new(tools: Vec<Arc<dyn crate::openhuman::tools::Tool>>) -> Self {
        Self { tools }
    }

    /// The advertised tool specs for the provider request.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }
}

#[async_trait]
impl LiveToolExecutor for HarnessToolExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> (String, bool) {
        let Some(tool) = self.tools.iter().find(|t| t.name() == name) else {
            tracing::warn!(tool = name, "[agent_graph::live] unknown tool requested");
            return (format!("Error: unknown tool '{name}'"), false);
        };
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        match tool.execute(args).await {
            Ok(result) => {
                let success = !result.is_error;
                (result.output_for_llm(true), success)
            }
            Err(e) => {
                tracing::warn!(tool = name, error = %e, "[agent_graph::live] tool execution failed");
                (format!("Error executing '{name}': {e}"), false)
            }
        }
    }
}

/// The live, non-serializable turn machinery, owned for the duration of one run
/// and shared with the graph nodes behind an async mutex.
pub struct LiveTurnMachine {
    /// Provider to call.
    pub provider: Arc<dyn Provider>,
    /// Model id.
    pub model: String,
    /// Sampling temperature.
    pub temperature: f64,
    /// Conversation so far (system + user + assistant + tool messages).
    pub history: Vec<ChatMessage>,
    /// Tool specs advertised to the model.
    pub tools: Vec<ToolSpec>,
    /// Tool executor.
    pub executor: Arc<dyn LiveToolExecutor>,
    /// Iteration cap.
    pub max_iterations: usize,

    // ── mutable run bookkeeping ──
    /// 1-based count of provider calls made.
    pub iteration: usize,
    /// Text of the most recent provider response.
    pub last_text: String,
    /// Tool calls in the most recent response.
    pub last_tool_calls: Vec<ToolCall>,
    /// Final assistant text once the loop terminates.
    pub final_text: String,
    /// Whether the run stopped by hitting the iteration cap.
    pub hit_cap: bool,
    /// Accumulated token/USD cost.
    pub cost: TurnCost,

    // ── Phase C parity seams ──
    /// Tool names that, on success, stop the loop early (e.g.
    /// `ask_user_clarification`) so the caller can pause/checkpoint.
    pub early_exit_tools: Vec<String>,
    /// Set when an early-exit tool fired.
    pub early_exit_tool: Option<String>,
    /// Repeated-failure circuit breaker (shared with the legacy loop).
    failure_guard: crate::openhuman::agent::harness::tool_loop::RepeatFailureGuard,
    /// No-progress (identical response+call) circuit breaker.
    repeat_guard: crate::openhuman::agent::harness::tool_loop::RepeatOutputGuard,
}

impl LiveTurnMachine {
    /// Build a machine for a turn. `history` should already contain the system
    /// prompt + the user message.
    pub fn new(
        provider: Arc<dyn Provider>,
        model: impl Into<String>,
        temperature: f64,
        history: Vec<ChatMessage>,
        tools: Vec<ToolSpec>,
        executor: Arc<dyn LiveToolExecutor>,
        max_iterations: usize,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
            history,
            tools,
            executor,
            max_iterations: max_iterations.max(1),
            iteration: 0,
            last_text: String::new(),
            last_tool_calls: Vec::new(),
            final_text: String::new(),
            hit_cap: false,
            cost: TurnCost::new(),
            early_exit_tools: Vec::new(),
            early_exit_tool: None,
            failure_guard: crate::openhuman::agent::harness::tool_loop::RepeatFailureGuard::new(),
            repeat_guard: crate::openhuman::agent::harness::tool_loop::RepeatOutputGuard::new(),
        }
    }

    /// Tools that stop the loop early on success (pause semantics).
    pub fn with_early_exit_tools(mut self, tools: Vec<String>) -> Self {
        self.early_exit_tools = tools;
        self
    }
}

/// The serializable, engine-visible state. Tiny on purpose — the heavy
/// machinery lives in the [`LiveTurnMachine`] behind the mutex.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LiveTurnState {
    /// 1-based provider-call count mirror (for checkpoints / observability).
    pub iteration: i64,
    /// Whether the turn has finished.
    pub done: bool,
}

impl GraphState for LiveTurnState {
    fn merge(&mut self, other: Self) -> Result<()> {
        // Linear chain → no real fan-out; take the most-advanced values.
        self.iteration = self.iteration.max(other.iteration);
        self.done = self.done || other.done;
        Ok(())
    }
}

/// Result of a live graph-driven turn.
#[derive(Debug, Clone)]
pub struct LiveTurnOutcome {
    /// Final assistant text.
    pub text: String,
    /// Provider calls made.
    pub iterations: u32,
    /// Whether the iteration cap was hit.
    pub hit_cap: bool,
    /// Accumulated cost.
    pub cost: TurnCost,
    /// Final conversation history (so callers can persist / checkpoint it).
    pub history: Vec<ChatMessage>,
    /// Set when the turn exited early because an early-exit tool fired (pause).
    pub early_exit_tool: Option<String>,
}

type Machine = Arc<Mutex<LiveTurnMachine>>;

/// Provider-call node: send the current history (+ tool specs) and capture the
/// response.
struct Dispatch(Machine);
#[async_trait]
impl Node<LiveTurnState> for Dispatch {
    async fn run(
        &self,
        mut state: LiveTurnState,
        ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<LiveTurnState>> {
        let mut m = self.0.lock().await;

        // ── Stop hooks (budget / max-iter policy) — legacy-loop parity. ──
        // Scoped so the immutable borrow of `m` ends before we mutate below.
        let mut stop_reason: Option<String> = None;
        {
            let hooks = crate::openhuman::agent::stop_hooks::current_stop_hooks();
            if !hooks.is_empty() {
                let st = crate::openhuman::agent::stop_hooks::TurnState {
                    iteration: (m.iteration + 1) as u32,
                    max_iterations: m.max_iterations as u32,
                    cost: &m.cost,
                    model: &m.model,
                };
                for hook in &hooks {
                    if let crate::openhuman::agent::stop_hooks::StopDecision::Stop { reason } =
                        hook.check(&st).await
                    {
                        stop_reason = Some(format!(
                            "Agent turn stopped by hook '{}': {reason}",
                            hook.name()
                        ));
                        break;
                    }
                }
            }
        }
        if let Some(reason) = stop_reason {
            tracing::warn!(reason = %reason, "[agent_graph::live] stop hook halted turn");
            m.final_text = reason;
            return Ok(NodeOutput::goto(state, "finalize"));
        }

        // Request copy of history, trimmed to the model's context window so a
        // long tool-calling chain can't overflow (legacy-loop parity). Trimming
        // the request copy (not persisted history) keeps the transcript intact.
        let mut messages = m.history.clone();
        if let Some(cw) = m.provider.effective_context_window(&m.model).await {
            let outcome =
                crate::openhuman::agent::harness::token_budget::trim_chat_messages_to_budget(
                    &mut messages,
                    cw,
                );
            if outcome.trimmed {
                tracing::debug!(
                    messages_removed = outcome.messages_removed,
                    "[agent_graph::live] pre-dispatch history trimmed to context window"
                );
            }
        }
        let use_native = m.provider.supports_native_tools() && !m.tools.is_empty();
        let tools = if use_native {
            Some(m.tools.clone())
        } else {
            None
        };
        tracing::debug!(
            run_id = ctx.run_id,
            iteration = m.iteration + 1,
            "[agent_graph::live] dispatch — calling provider"
        );
        let resp = m
            .provider
            .chat(
                ChatRequest {
                    messages: &messages,
                    tools: tools.as_deref(),
                    stream: None,
                    max_tokens: None,
                },
                &m.model,
                m.temperature,
            )
            .await?;
        if let Some(usage) = resp.usage.as_ref() {
            let model = m.model.clone();
            m.cost.add_call(&model, usage);
        }
        let text = resp.text_or_empty().to_string();
        m.last_text = text.clone();
        m.last_tool_calls = resp.tool_calls.clone();
        // Append the assistant turn. When the model emitted native tool calls,
        // serialize them into the assistant message via the SAME helper the
        // legacy loop uses, so the structured `tool_calls` + their ids survive
        // into the next provider request (otherwise OpenAI-compatible providers
        // 400 on the follow-up). Plain text when there are no calls. (Phase C
        // native/text history-threading parity, issue #4249.)
        let assistant_content = if resp.tool_calls.is_empty() {
            text
        } else {
            crate::openhuman::agent::harness::parse::build_native_assistant_history(
                &text,
                resp.reasoning_content.as_deref(),
                &resp.tool_calls,
            )
        };
        m.history.push(ChatMessage::assistant(assistant_content));
        m.iteration += 1;
        state.iteration = m.iteration as i64;
        Ok(NodeOutput::goto(state, "parse"))
    }
}

/// Parse node: final answer vs more tools vs iteration cap.
struct Parse(Machine);
#[async_trait]
impl Node<LiveTurnState> for Parse {
    async fn run(
        &self,
        state: LiveTurnState,
        _ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<LiveTurnState>> {
        let mut m = self.0.lock().await;
        if m.last_tool_calls.is_empty() {
            m.final_text = m.last_text.clone();
            return Ok(NodeOutput::goto(state, "finalize"));
        }
        if m.iteration >= m.max_iterations {
            m.hit_cap = true;
            m.final_text = if m.last_text.is_empty() {
                "Reached the iteration limit before producing a final answer.".to_string()
            } else {
                m.last_text.clone()
            };
            return Ok(NodeOutput::goto(state, "finalize"));
        }
        // No-progress circuit breaker: identical response + tool-call signature
        // repeated across iterations means the run is stuck. Mirrors the legacy
        // loop's `RepeatOutputGuard` check (issue #4249 Phase C parity).
        let mut sig = m.last_text.trim().to_string();
        for call in &m.last_tool_calls {
            sig.push('\u{1}');
            sig.push_str(&call.name);
            sig.push('\u{1}');
            sig.push_str(&call.arguments);
        }
        if let Some(reason) = m.repeat_guard.record(&sig) {
            tracing::warn!("[agent_graph::live] repeat-output breaker tripped — halting");
            m.final_text = reason;
            return Ok(NodeOutput::goto(state, "finalize"));
        }
        Ok(NodeOutput::goto(state, "tools"))
    }
}

/// Tool-execution node: run each requested call, append results, loop back.
struct Tools(Machine);
#[async_trait]
impl Node<LiveTurnState> for Tools {
    async fn run(
        &self,
        state: LiveTurnState,
        ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<LiveTurnState>> {
        let mut m = self.0.lock().await;
        let calls = m.last_tool_calls.clone();
        let mut halt: Option<String> = None;
        let mut early_exit: Option<String> = None;
        for call in &calls {
            tracing::debug!(
                run_id = ctx.run_id,
                tool = %call.name,
                "[agent_graph::live] executing tool"
            );
            let (output, success) = m.executor.execute(&call.name, &call.arguments).await;
            // Native tool-result message shape the legacy loop uses: one
            // `role: tool` message per call id, so the assistant `tool_calls`
            // and their results stay paired on the next request.
            let result = serde_json::json!({
                "tool_call_id": call.id,
                "content": output,
            });
            m.history.push(ChatMessage::tool(result.to_string()));

            // Repeated-failure circuit breaker (legacy-loop parity): halt with a
            // root cause instead of grinding to the iteration cap.
            if let Some(reason) =
                m.failure_guard
                    .record(&call.name, &call.arguments, success, &output)
            {
                tracing::warn!(tool = %call.name, "[agent_graph::live] circuit breaker tripped");
                halt = Some(reason);
                break;
            }
            // Early-exit tool (e.g. ask_user_clarification): stop so the caller
            // can pause/checkpoint and surface the question.
            if success && m.early_exit_tools.iter().any(|t| t == &call.name) {
                early_exit = Some(call.name.clone());
                m.final_text = output;
                break;
            }
        }
        if let Some(reason) = halt {
            m.final_text = reason;
            return Ok(NodeOutput::goto(state, "finalize"));
        }
        if let Some(tool) = early_exit {
            m.early_exit_tool = Some(tool);
            return Ok(NodeOutput::goto(state, "finalize"));
        }
        Ok(NodeOutput::goto(state, "dispatch"))
    }
}

/// Finalize node: terminal.
struct Finalize;
#[async_trait]
impl Node<LiveTurnState> for Finalize {
    async fn run(
        &self,
        mut state: LiveTurnState,
        _ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<LiveTurnState>> {
        state.done = true;
        Ok(NodeOutput::end(state))
    }
}

/// Build the canonical live-turn graph over a shared [`LiveTurnMachine`].
fn build_live_graph(
    machine: Machine,
) -> Result<crate::openhuman::agent_graph::graph::CompiledGraph<LiveTurnState>, GraphError> {
    let mut g = StateGraph::<LiveTurnState>::new("live_turn");
    g.add_node("dispatch", Arc::new(Dispatch(machine.clone())));
    g.add_node("parse", Arc::new(Parse(machine.clone())));
    g.add_node("tools", Arc::new(Tools(machine)));
    g.add_node("finalize", Arc::new(Finalize));
    g.set_entry_point("dispatch");
    // Nodes route via `Goto`; the static spine keeps every node non-dangling.
    g.add_edge("dispatch", "parse");
    g.add_edge("parse", "tools");
    g.add_edge("tools", "dispatch");
    g.set_finish_point("finalize");
    g.compile()
}

/// Run a real agent turn by driving the `agent_graph` engine over a live
/// provider + tool loop. The crux of Phase A: the state machine, not a
/// hand-written `for` loop, owns the turn's control flow.
pub async fn run_turn_via_graph(machine: LiveTurnMachine) -> Result<LiveTurnOutcome> {
    let machine = Arc::new(Mutex::new(machine));
    let graph = build_live_graph(machine.clone())
        .map_err(|e| anyhow::anyhow!("live turn graph failed to compile: {e}"))?;
    graph
        .invoke(LiveTurnState::default())
        .await
        .map_err(|e| anyhow::anyhow!("live turn run failed: {e}"))?;
    let m = machine.lock().await;
    tracing::info!(
        iterations = m.iteration,
        hit_cap = m.hit_cap,
        "[agent_graph::live] turn complete"
    );
    Ok(LiveTurnOutcome {
        text: m.final_text.clone(),
        iterations: m.iteration as u32,
        hit_cap: m.hit_cap,
        cost: m.cost.clone(),
        history: m.history.clone(),
        early_exit_tool: m.early_exit_tool.clone(),
    })
}

/// A [`LiveToolExecutor`] over one or more shared harness tool sets
/// (`Arc<Vec<Box<dyn Tool>>>`, the shape the sub-agent runner already owns via
/// `ParentExecutionContext.all_tools`), with an optional name allowlist. Because
/// the sources are `Arc`, the executor is `'static` and needs no engine
/// lifetime change. This is what lets the sub-agent path run on the graph
/// engine (Phase B).
pub struct SharedToolExecutor {
    sources: Vec<Arc<Vec<Box<dyn crate::openhuman::tools::Tool>>>>,
    allowed: std::collections::HashSet<String>,
}

impl SharedToolExecutor {
    /// Build from shared tool sources + an allowlist (empty = allow all).
    pub fn new(
        sources: Vec<Arc<Vec<Box<dyn crate::openhuman::tools::Tool>>>>,
        allowed: std::collections::HashSet<String>,
    ) -> Self {
        Self { sources, allowed }
    }
}

#[async_trait]
impl LiveToolExecutor for SharedToolExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> (String, bool) {
        if !self.allowed.is_empty() && !self.allowed.contains(name) {
            return (
                format!("Error: tool '{name}' is not permitted for this agent"),
                false,
            );
        }
        let tool = self
            .sources
            .iter()
            .flat_map(|s| s.iter())
            .find(|t| t.name() == name);
        let Some(tool) = tool else {
            tracing::warn!(tool = name, "[agent_graph::live] unknown tool requested");
            return (format!("Error: unknown tool '{name}'"), false);
        };
        let args: serde_json::Value = serde_json::from_str(arguments)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        match tool.execute(args).await {
            Ok(result) => (result.output_for_llm(true), !result.is_error),
            Err(e) => (format!("Error executing '{name}': {e}"), false),
        }
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod live_tests;
