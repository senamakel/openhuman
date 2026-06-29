//! The typed working state that flows through a graph.

use anyhow::Result;

/// State carried across nodes and super-steps.
///
/// This is the "persistent working state" issue #4249 asks for: variables and
/// intermediate results that survive node transitions, parallel branches, and
/// (via the checkpointer) context compaction / pause-resume.
///
/// Requirements:
/// - `Clone` so the executor can hand a copy to each parallel branch.
/// - `Serialize`/`DeserializeOwned` so the checkpointer can persist + restore it.
/// - [`merge`](GraphState::merge) so the executor can fold the results of
///   parallel branches back into one state (LangGraph's channel-reducer role).
///   For a purely linear graph `merge` is never called, so a trivial
///   last-writer-wins impl is fine.
pub trait GraphState:
    Clone + serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static
{
    /// Fold `other` (the result of a sibling parallel branch) into `self`.
    ///
    /// Called once per extra branch when a [`Command::Fork`](crate::openhuman::agent_graph::types::Command::Fork)
    /// fans out and the branches rejoin (or when several branches converge on
    /// the same node / `END`). Implementations decide the reducer semantics
    /// (append lists, take max, last-writer-wins, …). Returning `Err` aborts the
    /// run with [`GraphError::MergeFailed`](crate::openhuman::agent_graph::types::GraphError::MergeFailed).
    ///
    /// **Fan-out semantics:** each parallel branch is handed a *full clone* of
    /// the pre-fork state, not a blank delta. So a field already populated
    /// before the fork is present in every branch and will be seen again here.
    /// Reducers that must not double-count a shared base should be written
    /// accordingly (e.g. last-writer-wins on scalar fields, set-union on
    /// collections), or the graph should write per-branch results into distinct
    /// fields and combine them in a dedicated join node.
    fn merge(&mut self, other: Self) -> Result<()>;
}
