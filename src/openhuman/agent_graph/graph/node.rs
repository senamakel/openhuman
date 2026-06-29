//! Node trait, execution context, and the observability/cancellation seams the
//! executor threads into every node.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::state::GraphState;
use crate::openhuman::agent_graph::types::Command;

/// What a node returns: the (possibly mutated) state plus a routing [`Command`].
///
/// Nodes take the state by value and hand it back so parallel branches can each
/// own a clone without aliasing a single `&mut` — the Pregel "state in, state
/// out" shape.
pub struct NodeOutput<S> {
    /// State after the node ran.
    pub state: S,
    /// Where to go next.
    pub command: Command,
}

impl<S> NodeOutput<S> {
    /// Convenience: mutated state, follow the static edge.
    pub fn cont(state: S) -> Self {
        Self {
            state,
            command: Command::Continue,
        }
    }

    /// Convenience: mutated state, jump to `node`.
    pub fn goto(state: S, node: impl Into<String>) -> Self {
        Self {
            state,
            command: Command::Goto(node.into()),
        }
    }

    /// Convenience: mutated state, end this branch.
    pub fn end(state: S) -> Self {
        Self {
            state,
            command: Command::End,
        }
    }
}

/// A unit of work in the graph.
///
/// Nodes are registered by name in the builder; the name is the routing key, so
/// the trait itself carries no id. Anything a node needs at runtime (provider,
/// tool registry, config) it captures in its own fields — typically `Arc`
/// clones — keeping the engine decoupled from the agent harness.
#[async_trait]
pub trait Node<S: GraphState>: Send + Sync {
    /// Run the node. `state` is owned so parallel branches don't alias.
    async fn run(&self, state: S, ctx: &NodeCtx<'_>) -> Result<NodeOutput<S>>;
}

/// Cooperative cancellation token shared across a run (and clonable into
/// spawned branches). Flipping it makes the executor bail at the next
/// super-step boundary with [`GraphError::Cancelled`](crate::openhuman::agent_graph::types::GraphError::Cancelled).
#[derive(Clone, Default)]
pub struct GraphCancel(Arc<AtomicBool>);

impl GraphCancel {
    /// Fresh, un-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Sink for graph lifecycle events. The executor calls these around every node;
/// the default no-op impl ([`NullSink`]) lets tests run without wiring
/// observability. The production impl in
/// [`crate::openhuman::agent_graph::observability`] bridges to `AgentProgress`
/// and the event bus.
pub trait ProgressSink: Send + Sync {
    /// A node is about to run.
    fn node_entered(&self, _run_id: &str, _step: u32, _node: &str) {}
    /// A node finished, with the rendered command label.
    fn node_completed(
        &self,
        _run_id: &str,
        _step: u32,
        _node: &str,
        _command: &str,
        _elapsed_ms: u64,
    ) {
    }
    /// The run paused at an interrupt.
    fn run_paused(&self, _run_id: &str, _node: &str, _kind: &str) {}
}

/// No-op [`ProgressSink`] for tests and headless runs.
pub struct NullSink;
impl ProgressSink for NullSink {}

/// Per-run context handed to every node.
///
/// Engine-level only — no agent-harness types — so the core stays testable in
/// isolation. Nodes that need harness services capture them in their own
/// struct.
pub struct NodeCtx<'a> {
    /// Stable id for this run (also the checkpoint key).
    pub run_id: &'a str,
    /// Name of the graph definition being run.
    pub graph_name: &'a str,
    /// 0-based super-step index.
    pub step: u32,
    /// Observability sink.
    pub sink: &'a dyn ProgressSink,
    /// Cooperative cancellation.
    pub cancel: &'a GraphCancel,
}
