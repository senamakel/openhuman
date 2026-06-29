//! Core value types for the `agent_graph` state-machine engine.
//!
//! These are intentionally free of any agent-harness coupling so the engine in
//! [`crate::openhuman::agent_graph::graph`] can be unit-tested in isolation. The
//! concrete agent integration lives in
//! [`crate::openhuman::agent_graph::definitions`].

use serde::{Deserialize, Serialize};

/// Identifier for a node in a [`StateGraph`](crate::openhuman::agent_graph::graph::StateGraph).
pub type NodeId = String;

/// Reserved sink node every finished branch routes into. A node returning
/// [`Command::Goto`] to this id (or [`Command::End`]) terminates that branch.
pub const END: &str = "__end__";

/// Reserved virtual source. Not a real node — the builder's entry point is the
/// first real node the engine schedules.
pub const START: &str = "__start__";

/// What a node asks the executor to do after it runs.
///
/// Mirrors LangGraph's `Command` / conditional-edge return: a node can fall
/// through to its statically-wired successor, jump somewhere explicit, fan out
/// to several branches, pause for a human, or end its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Follow the node's statically-declared edge (set via `add_edge` /
    /// `add_conditional_edges`). The common case.
    Continue,
    /// Override static routing and jump straight to `node`.
    Goto(NodeId),
    /// Fan out to every listed node in the next super-step. Their resulting
    /// states are merged back via [`GraphState::merge`].
    Fork(Vec<NodeId>),
    /// Pause the run for human (or external) input. The executor writes a
    /// checkpoint and returns with [`GraphRunStatus::Paused`].
    Interrupt(InterruptRequest),
    /// Terminate this branch (equivalent to `Goto(END)`).
    End,
}

/// A request surfaced to a human when a node returns [`Command::Interrupt`].
///
/// Carries the question/options to show and where to resume once the input
/// arrives. Persisted inside the paused checkpoint so a later
/// `agent_graph_resume` RPC can continue the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptRequest {
    /// Stable kind, e.g. `"approval"`, `"clarification"`. Lets the UI pick a
    /// surface.
    pub kind: String,
    /// Human-facing prompt.
    pub question: String,
    /// Optional choices to present (empty = free-form input).
    #[serde(default)]
    pub options: Vec<String>,
    /// Node to schedule when the run resumes. Defaults to the interrupting
    /// node's static successor when `None`.
    #[serde(default)]
    pub resume_to: Option<NodeId>,
}

/// Lifecycle status of a graph run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphRunStatus {
    /// Created but not yet started (persisted before the first super-step).
    Pending,
    /// Actively executing.
    Running,
    /// Reached a finish node / `END`.
    Completed,
    /// Paused at a human-in-the-loop interrupt; resumable.
    Paused,
    /// Aborted by an error or a guard (depth / step cap).
    Failed,
}

impl GraphRunStatus {
    /// String form used for SQLite columns + RPC payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            GraphRunStatus::Pending => "pending",
            GraphRunStatus::Running => "running",
            GraphRunStatus::Completed => "completed",
            GraphRunStatus::Paused => "paused",
            GraphRunStatus::Failed => "failed",
        }
    }

    /// Parse the string form. Unknown values map to `Failed` (fail closed).
    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => GraphRunStatus::Pending,
            "running" => GraphRunStatus::Running,
            "completed" => GraphRunStatus::Completed,
            "paused" => GraphRunStatus::Paused,
            _ => GraphRunStatus::Failed,
        }
    }

    /// Whether the run is finished (no further super-steps possible without a
    /// resume).
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            GraphRunStatus::Completed | GraphRunStatus::Failed | GraphRunStatus::Paused
        )
    }
}

/// One recorded node visit, persisted for observability / `agent_graph_run_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeTransition {
    /// 0-based super-step index this transition belongs to.
    pub step: u32,
    /// Node that ran.
    pub node: NodeId,
    /// The command it returned, rendered as a short label (`continue`,
    /// `goto:foo`, `fork:[a,b]`, `interrupt:approval`, `end`).
    pub command: String,
    /// Wall-clock the node took, milliseconds.
    pub elapsed_ms: u64,
    /// RFC-3339 timestamp the node completed.
    pub ts: String,
}

/// Outcome of a full [`CompiledGraph::invoke`](crate::openhuman::agent_graph::graph::CompiledGraph)
/// (or resume).
#[derive(Debug, Clone)]
pub struct RunResult<S> {
    /// Final (or paused) merged state.
    pub state: S,
    /// Terminal status.
    pub status: GraphRunStatus,
    /// Last node that executed.
    pub last_node: NodeId,
    /// Number of super-steps executed.
    pub steps: u32,
    /// Set when `status == Paused`: the interrupt that halted the run.
    pub interrupt: Option<InterruptRequest>,
    /// Per-node transitions in execution order.
    pub transitions: Vec<NodeTransition>,
}

/// Errors the engine can raise. Builder/compile errors are distinct from
/// runtime guard trips so callers and tests can assert on the cause.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// `compile()` ran with no entry point set.
    #[error("graph has no entry point")]
    NoEntryPoint,
    /// An edge or entry references a node that was never added.
    #[error("graph references unknown node '{0}'")]
    UnknownNode(NodeId),
    /// A node has no outgoing edge and is not a finish node, so the executor
    /// would not know where to go after it returns `Command::Continue`.
    #[error("node '{0}' has no outgoing edge and is not a finish node")]
    DanglingNode(NodeId),
    /// The run exceeded the configured super-step cap (runaway-cycle guard).
    #[error("graph exceeded max steps ({0})")]
    MaxStepsExceeded(u32),
    /// A node body returned an error.
    #[error("node '{node}' failed: {source}")]
    NodeFailed {
        /// The node that failed.
        node: NodeId,
        /// The underlying error.
        #[source]
        source: anyhow::Error,
    },
    /// [`GraphState::merge`] rejected a parallel-branch merge.
    #[error("state merge failed: {0}")]
    MergeFailed(#[source] anyhow::Error),
    /// Run was cancelled via the engine's cancellation token.
    #[error("graph run cancelled")]
    Cancelled,
}
