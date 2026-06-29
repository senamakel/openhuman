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

/// Executes one tool call. The real adapter (Phase B) wraps the harness tool
/// registry; tests supply a deterministic mock.
#[async_trait]
pub trait LiveToolExecutor: Send + Sync {
    /// Run `name(arguments)` and return `(output, success)`.
    async fn execute(&self, name: &str, arguments: &str) -> (String, bool);
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
        }
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
        let messages = m.history.clone();
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
        // Append the assistant turn. (Phase A keeps this simple; the full
        // native/text history threading is reused when the real ToolSource is
        // wired in Phase B.)
        m.history.push(ChatMessage::assistant(text));
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
        for call in &calls {
            tracing::debug!(
                run_id = ctx.run_id,
                tool = %call.name,
                "[agent_graph::live] executing tool"
            );
            let (output, success) = m.executor.execute(&call.name, &call.arguments).await;
            let result = serde_json::json!({
                "tool_call_id": call.id,
                "content": output,
                "success": success,
            });
            m.history.push(ChatMessage::tool(result.to_string()));
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
    })
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod live_tests;
