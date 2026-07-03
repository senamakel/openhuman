//! The single orchestration wake graph (stage 4).
//!
//! The whole wake path is **one** `tinyagents` [`CompiledGraph`] (mirroring the
//! spec's one `StateGraph`), not agents ping-ponging through the event bus:
//!
//! ```text
//!   normalize ─► frontend ──(instructions)──► execute ─┐
//!                   ▲                                    │
//!                   └────────────────────────────────────┘
//!                   │
//!                   └──(channel_response)──► send_dm ─► context_guard ─► done
//! ```
//!
//! One [`OrchestrationState`] flows through conditional edges. The router is a
//! single command-routing node (`frontend`): `channel_response` present → wrap
//! up (`send_dm`), else → `execute` and back. That predicate **must** terminate
//! (spec §5 loop continuity) — it does, because `execute` always sets
//! `agent_reply`, so the second `frontend` pass compiles a `channel_response`;
//! a hard `max_supersteps` backstop forces a terminal reply if it ever does not.
//!
//! The three behaviour-bearing nodes — the two-pass front end (Quick LLM), the
//! reasoning core (stubbed this stage), and the DM sender — are **injected** as
//! trait objects so the graph mechanics (routing + termination) are unit-testable
//! with trivial stubs, while production wires the real front-end agent
//! (`hint:chat`) and the tiny.place Signal reply. See [`super::ops`].

pub mod state;

use std::sync::Arc;

use async_trait::async_trait;
use tinyagents::graph::export::GraphTopology;
use tinyagents::graph::{
    ClosureStateReducer, Command, CompiledGraph, GraphBuilder, NodeContext, NodeResult,
};

use crate::openhuman::config::Config;
use crate::openhuman::tinyagents::observability::GraphTracingSink;
use crate::openhuman::tinyagents::SqlRunLedgerCheckpointer;

pub use state::{CompressedEntry, OrchestrationState, WorldDiff, WorldDiffEntry};

const LOG: &str = "orchestration";

/// Rough denominator for `context_utilization` until stage 5 wires the real
/// token-window accounting. Keeps the field populated (0.0–1.0) before END.
const CONTEXT_MESSAGE_BUDGET: f32 = 200.0;

/// The two-pass Quick-LLM front end. Pass 1 turns raw session/master traffic
/// into macro-instructions for the reasoning core; pass 2 compiles the
/// reasoning reply into the finished channel response.
#[async_trait]
pub trait FrontendNode: Send + Sync {
    /// Pass 1 — raw traffic → macro-instructions for the reasoning core.
    async fn instruct(&self, state: &OrchestrationState) -> anyhow::Result<String>;
    /// Pass 2 — reasoning reply → finished channel-response text.
    async fn compile_reply(&self, state: &OrchestrationState) -> anyhow::Result<String>;
}

/// The reasoning core. Stubbed this stage (sets a canned `agent_reply`);
/// replaced by the real sub-agent-spawning `execute` node in stage 5.
#[async_trait]
pub trait ReasoningNode: Send + Sync {
    async fn execute(&self, state: &OrchestrationState) -> anyhow::Result<String>;
}

/// Sends the compiled `channel_response` back to the originating tiny.place DM.
#[async_trait]
pub trait ChannelSender: Send + Sync {
    async fn send_dm(&self, counterpart_agent_id: &str, body: &str) -> anyhow::Result<()>;
}

/// Reducer update emitted by an orchestration node. Exactly one per node result
/// (the crate applies a single `Update` per boundary), so combined field writes
/// live in a single variant. Public only because it appears in the
/// [`CompiledGraph`] type parameter [`build_orchestration_graph`] returns; the
/// variants are an internal reducer detail.
pub enum OrchestrationUpdate {
    /// Front-end pass 1: store macro-instructions + advance the pass counter.
    Pass1 { instructions: String },
    /// Front-end pass 2: store the finished channel response + advance the pass
    /// counter. Its presence is the terminate predicate.
    Pass2 { channel_response: String },
    /// Reasoning core produced a reply.
    Reply(String),
    /// The outbound DM was dispatched — latch so it can never double-send.
    DmSent,
    /// Context guard computed utilization.
    Context(f32),
    /// No state change (structural nodes).
    Noop,
}

/// Lift an injected node's `anyhow` error into the graph error type so a failure
/// fails the run rather than silently stalling.
fn graph_err(e: anyhow::Error) -> tinyagents::TinyAgentsError {
    tinyagents::TinyAgentsError::Graph(e.to_string())
}

fn compute_utilization(state: &OrchestrationState) -> f32 {
    (state.messages.len() as f32 / CONTEXT_MESSAGE_BUDGET).min(1.0)
}

/// Build (but do not run) the orchestration wake graph. Shared by
/// [`run_orchestration_graph`] and [`orchestration_graph_topology`] so the
/// structure has one definition.
///
/// `max_supersteps` is the loop-continuity backstop: if the front-end pass count
/// ever exceeds it, the node force-compiles a terminal `channel_response` and
/// routes to `send_dm` instead of looping back to `execute`.
pub fn build_orchestration_graph(
    frontend: Arc<dyn FrontendNode>,
    reasoning: Arc<dyn ReasoningNode>,
    sender: Arc<dyn ChannelSender>,
    max_supersteps: u32,
) -> anyhow::Result<CompiledGraph<OrchestrationState, OrchestrationUpdate>> {
    let mut builder = GraphBuilder::<OrchestrationState, OrchestrationUpdate>::new().set_reducer(
        ClosureStateReducer::new(|mut s: OrchestrationState, u: OrchestrationUpdate| {
            match u {
                OrchestrationUpdate::Pass1 { instructions } => {
                    s.agent_instructions = Some(instructions);
                    s.pass += 1;
                }
                OrchestrationUpdate::Pass2 { channel_response } => {
                    s.channel_response = Some(channel_response);
                    s.pass += 1;
                }
                OrchestrationUpdate::Reply(reply) => s.agent_reply = Some(reply),
                OrchestrationUpdate::DmSent => s.dm_sent = true,
                OrchestrationUpdate::Context(util) => s.context_utilization = util,
                OrchestrationUpdate::Noop => {}
            }
            Ok(s)
        }),
    );

    // `normalize`: the recent-message window is folded into state before the run
    // (see `ops::seed_state`), so this node is a structural entry that logs and
    // hands off to the front end.
    builder = builder.add_node(
        "normalize",
        |s: OrchestrationState, _c: NodeContext| async move {
            tracing::debug!(
                target: LOG,
                session_id = %s.session_id,
                node = "normalize",
                messages = s.messages.len(),
                "[orchestration] node.enter",
            );
            Ok(NodeResult::Update(OrchestrationUpdate::Noop))
        },
    );

    // `frontend`: the router. Two-pass, Quick LLM, conditional goto.
    builder = builder.add_node("frontend", move |s: OrchestrationState, _c: NodeContext| {
        let frontend = frontend.clone();
        async move {
            let pass = s.pass + 1;

            // Defensive terminate: a response already exists (re-entry / resume)
            // — route straight to send_dm without re-calling the LLM.
            if s.channel_response.is_some() {
                tracing::debug!(
                    target: LOG, session_id = %s.session_id, node = "frontend", pass,
                    route = "send_dm", reason = "channel_response_present",
                    "[orchestration] node.route",
                );
                return Ok(NodeResult::Command(
                    Command::default().with_goto(["send_dm"]),
                ));
            }

            // Loop-continuity backstop (spec §5): never cycle past the cap.
            if pass > max_supersteps {
                let body = s.agent_reply.clone().unwrap_or_else(|| "…".to_string());
                tracing::warn!(
                    target: LOG, session_id = %s.session_id, node = "frontend", pass,
                    route = "send_dm", reason = "max_supersteps_backstop",
                    "[orchestration] node.route",
                );
                return Ok(NodeResult::Command(
                    Command::default()
                        .with_update(OrchestrationUpdate::Pass2 {
                            channel_response: body,
                        })
                        .with_goto(["send_dm"]),
                ));
            }

            // Pass 2: the reasoning core has replied → compile the channel response.
            if s.agent_reply.is_some() {
                let body = frontend.compile_reply(&s).await.map_err(graph_err)?;
                tracing::debug!(
                    target: LOG, session_id = %s.session_id, node = "frontend", pass,
                    route = "send_dm", reason = "reply_ready",
                    "[orchestration] node.route",
                );
                return Ok(NodeResult::Command(
                    Command::default()
                        .with_update(OrchestrationUpdate::Pass2 {
                            channel_response: body,
                        })
                        .with_goto(["send_dm"]),
                ));
            }

            // Pass 1: raw traffic → macro-instructions, hand down to the core.
            let instructions = frontend.instruct(&s).await.map_err(graph_err)?;
            tracing::debug!(
                target: LOG, session_id = %s.session_id, node = "frontend", pass,
                route = "execute", reason = "first_pass",
                "[orchestration] node.route",
            );
            Ok(NodeResult::Command(
                Command::default()
                    .with_update(OrchestrationUpdate::Pass1 { instructions })
                    .with_goto(["execute"]),
            ))
        }
    });

    // `execute`: the reasoning core (stubbed) — sets `agent_reply`, loops back
    // to the front end for pass 2.
    builder = builder.add_node("execute", move |s: OrchestrationState, _c: NodeContext| {
        let reasoning = reasoning.clone();
        async move {
            let reply = reasoning.execute(&s).await.map_err(graph_err)?;
            tracing::debug!(
                target: LOG, session_id = %s.session_id, node = "execute",
                "[orchestration] node.exit",
            );
            Ok(NodeResult::Update(OrchestrationUpdate::Reply(reply)))
        }
    });

    // `send_dm`: the outbound Signal reply. Sends at most once — the `dm_sent`
    // latch survives checkpoint/resume so a re-entered cycle never double-sends.
    builder = builder.add_node("send_dm", move |s: OrchestrationState, _c: NodeContext| {
        let sender = sender.clone();
        async move {
            if s.dm_sent {
                tracing::debug!(
                    target: LOG, session_id = %s.session_id, node = "send_dm",
                    reason = "already_sent", "[orchestration] node.skip",
                );
            } else if let Some(body) = s.channel_response.as_deref() {
                sender
                    .send_dm(&s.counterpart_agent_id, body)
                    .await
                    .map_err(graph_err)?;
                tracing::debug!(
                    target: LOG, session_id = %s.session_id, node = "send_dm",
                    counterpart = %s.counterpart_agent_id, "[orchestration] node.sent",
                );
            }
            Ok(NodeResult::Update(OrchestrationUpdate::DmSent))
        }
    });

    // `context_guard`: compute utilization before END (stage 5 adds eviction).
    builder = builder.add_node(
        "context_guard",
        |s: OrchestrationState, _c: NodeContext| async move {
            let util = compute_utilization(&s);
            tracing::debug!(
                target: LOG, session_id = %s.session_id, node = "context_guard",
                context_utilization = util, "[orchestration] node.exit",
            );
            Ok(NodeResult::Update(OrchestrationUpdate::Context(util)))
        },
    );

    let graph = builder
        .add_node(
            "done",
            |_s: OrchestrationState, _c: NodeContext| async move {
                Ok(NodeResult::Update(OrchestrationUpdate::Noop))
            },
        )
        .add_edge("normalize", "frontend")
        .add_edge("execute", "frontend")
        .add_edge("send_dm", "context_guard")
        .add_edge("context_guard", "done")
        .set_entry("normalize")
        .mark_command_routing("frontend")
        .set_finish("done")
        .compile()
        .map_err(|e| anyhow::anyhow!("orchestration graph compile failed: {e}"))?;
    Ok(graph)
}

/// Drive one wake cycle for `state.session_id` on the orchestration graph,
/// checkpointing every super-step boundary under thread
/// `orchestration:<session_id>`. Returns the terminal state.
pub async fn run_orchestration_graph(
    config: Arc<Config>,
    frontend: Arc<dyn FrontendNode>,
    reasoning: Arc<dyn ReasoningNode>,
    sender: Arc<dyn ChannelSender>,
    state: OrchestrationState,
) -> anyhow::Result<OrchestrationState> {
    let max = config.orchestration.max_supersteps;
    let thread_id = format!("orchestration:{}", state.session_id);
    let label = thread_id.clone();
    let checkpointer = Arc::new(SqlRunLedgerCheckpointer::<OrchestrationState>::new(config));

    tracing::debug!(
        target: LOG,
        session_id = %state.session_id,
        %thread_id,
        messages = state.messages.len(),
        "[orchestration] graph.run.enter",
    );

    let graph = build_orchestration_graph(frontend, reasoning, sender, max)?
        .with_checkpointer(checkpointer)
        .with_event_sink(Arc::new(GraphTracingSink::new(label)));

    let exec = graph
        .run_with_thread(thread_id, state)
        .await
        .map_err(|e| anyhow::anyhow!("orchestration graph run failed: {e}"))?;

    tracing::debug!(
        target: LOG,
        session_id = %exec.state.session_id,
        steps = exec.steps,
        dm_sent = exec.state.dm_sent,
        pass = exec.state.pass,
        "[orchestration] graph.run.exit",
    );
    Ok(exec.state)
}

/// Structure-only [`GraphTopology`] of the wake graph for debug / inspection
/// (issue #4249, Phase 4). Built with no-op stub handlers — exposes only node
/// names, edges, and routing, never handler bodies.
pub fn orchestration_graph_topology() -> anyhow::Result<GraphTopology> {
    struct NoopFrontend;
    #[async_trait]
    impl FrontendNode for NoopFrontend {
        async fn instruct(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn compile_reply(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }
    struct NoopReasoning;
    #[async_trait]
    impl ReasoningNode for NoopReasoning {
        async fn execute(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }
    struct NoopSender;
    #[async_trait]
    impl ChannelSender for NoopSender {
        async fn send_dm(&self, _c: &str, _b: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    let graph = build_orchestration_graph(
        Arc::new(NoopFrontend),
        Arc::new(NoopReasoning),
        Arc::new(NoopSender),
        12,
    )?;
    Ok(graph.topology())
}

#[cfg(test)]
mod tests;
