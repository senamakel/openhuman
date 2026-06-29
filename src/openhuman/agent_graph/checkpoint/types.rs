//! Persisted shapes for graph runs + checkpoints.
//!
//! State is stored type-erased as `serde_json::Value` (the serialized
//! [`GraphState`](crate::openhuman::agent_graph::graph::GraphState)) so the
//! storage layer stays generic over the concrete state type.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::openhuman::agent_graph::types::{GraphRunStatus, InterruptRequest, NodeTransition};

/// One row in the runs ledger — the durable record of a graph invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRunRecord {
    /// Stable run id (checkpoint key).
    pub run_id: String,
    /// Graph definition name.
    pub graph_name: String,
    /// Current lifecycle status.
    pub status: GraphRunStatus,
    /// RFC-3339 creation time.
    pub created_at: String,
    /// RFC-3339 last-update time.
    pub updated_at: String,
    /// Last node that executed.
    pub last_node: String,
    /// Super-steps executed so far.
    pub steps: u32,
    /// Latest serialized state (the resume payload).
    pub state: Value,
    /// Set when `status == Paused`: the interrupt that halted the run.
    #[serde(default)]
    pub interrupt: Option<InterruptRequest>,
    /// Set when `status == Failed`.
    #[serde(default)]
    pub error: Option<String>,
    /// Node transitions recorded so far (observability).
    #[serde(default)]
    pub transitions: Vec<NodeTransition>,
}

/// A point-in-time state snapshot for a run. Multiple per run (one per save).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Auto-assigned id (0 until persisted).
    #[serde(default)]
    pub id: i64,
    /// Owning run.
    pub run_id: String,
    /// Super-step the snapshot was taken at.
    pub step: u32,
    /// Node the run was at.
    pub node: String,
    /// Why the checkpoint was written, e.g. `"start"`, `"pause:approval"`,
    /// `"complete"`.
    pub label: String,
    /// Serialized state at this point.
    pub state: Value,
    /// RFC-3339 timestamp.
    pub created_at: String,
}
