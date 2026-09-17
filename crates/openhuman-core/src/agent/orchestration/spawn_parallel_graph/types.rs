//! Shared worker and result types passed between the staging, worker-fanout,
//! and collection phases.

use std::path::PathBuf;

use serde::Serialize;
use tinyagents_harness::workspace::WorkspaceDescriptor;

use super::request::ParallelAgentTask;
use crate::agent::harness::definition::AgentDefinition;

use super::staging::WorkerDispatchMode;

/// A staged worker with everything the fanout needs: resolved definition,
/// prompt (with any ownership boundary applied), and worktree placement.
#[derive(Clone)]
pub(crate) struct SpawnParallelWorker {
    pub(crate) definition: AgentDefinition,
    pub(crate) prompt: String,
    pub(crate) task: ParallelAgentTask,
    pub(crate) task_id: String,
    pub(crate) lineage: ParallelAgentLineage,
    pub(crate) worktree_path: Option<PathBuf>,
    pub(crate) workspace_descriptor: Option<WorkspaceDescriptor>,
    pub(crate) dispatch_mode: WorkerDispatchMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParallelAgentLineage {
    pub(crate) parent_session: String,
    pub(crate) root_session: String,
    pub(crate) child_task_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParallelAgentResult {
    pub(crate) task_id: String,
    pub(crate) agent_id: String,
    pub(crate) lineage: ParallelAgentLineage,
    pub(crate) success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ownership: Option<String>,
    pub(crate) elapsed_ms: u64,
    pub(crate) iterations: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) stale_parent_reads: Vec<String>,
    /// Absolute path to the worker's isolated `git worktree` checkout, when
    /// it ran with `isolation = "worktree"`. `None` for non-isolated workers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) worktree_path: Option<String>,
    /// Files (relative to the worktree root) the worker changed, collected
    /// from `git status` after the run. Empty for non-isolated workers or a
    /// clean worktree.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) changed_files: Vec<String>,
    /// Whether the worker's worktree had uncommitted changes after the run.
    /// A dirty worktree must not be auto-removed (surfaced to the UI so the
    /// user can choose). `None` for non-isolated workers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dirty_status: Option<bool>,
}
