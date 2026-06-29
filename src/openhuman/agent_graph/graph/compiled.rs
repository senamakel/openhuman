//! Public run/resume entry points on [`CompiledGraph`].

use super::builder::CompiledGraph;
use super::executor::run_from;
use super::node::{GraphCancel, NullSink, ProgressSink};
use super::state::GraphState;
use crate::openhuman::agent_graph::types::{GraphError, NodeId, RunResult};

impl<S: GraphState> CompiledGraph<S> {
    /// Run the graph from its entry point with `init` state.
    ///
    /// Returns when every branch ends ([`GraphRunStatus::Completed`]), a node
    /// pauses for a human ([`GraphRunStatus::Paused`]), or a guard trips
    /// (error). The simplest invocation; tests and headless callers use it.
    ///
    /// [`GraphRunStatus::Completed`]: crate::openhuman::agent_graph::types::GraphRunStatus::Completed
    /// [`GraphRunStatus::Paused`]: crate::openhuman::agent_graph::types::GraphRunStatus::Paused
    pub async fn invoke(&self, init: S) -> Result<RunResult<S>, GraphError> {
        let run_id = uuid::Uuid::new_v4().to_string();
        self.invoke_with(init, &run_id, &NullSink, &GraphCancel::new())
            .await
    }

    /// Run from the entry point with full control over `run_id`, the
    /// observability `sink`, and a `cancel` token. Used by the production
    /// definitions + RPC layer.
    pub async fn invoke_with(
        &self,
        init: S,
        run_id: &str,
        sink: &dyn ProgressSink,
        cancel: &GraphCancel,
    ) -> Result<RunResult<S>, GraphError> {
        tracing::info!(
            run_id,
            graph = %self.name,
            entry = %self.entry,
            "[agent_graph] invoke"
        );
        let frontier = vec![(self.entry.clone(), init)];
        run_from(self, frontier, run_id, sink, cancel, 0).await
    }

    /// Resume a paused run: continue from `resume_node` with the restored
    /// `state` (which the caller has already updated with the human's input).
    pub async fn resume_with(
        &self,
        state: S,
        resume_node: NodeId,
        run_id: &str,
        sink: &dyn ProgressSink,
        cancel: &GraphCancel,
        start_step: u32,
    ) -> Result<RunResult<S>, GraphError> {
        if resume_node != crate::openhuman::agent_graph::types::END
            && !self.nodes.contains_key(&resume_node)
        {
            return Err(GraphError::UnknownNode(resume_node));
        }
        tracing::info!(
            run_id,
            graph = %self.name,
            resume_node = %resume_node,
            start_step,
            "[agent_graph] resume"
        );
        let frontier = vec![(resume_node, state)];
        run_from(self, frontier, run_id, sink, cancel, start_step).await
    }
}
