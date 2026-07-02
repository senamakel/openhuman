//! Sub-agent pipeline graph scaffold for the TinyAgents migration.
//!
//! This fixed topology names the sub-agent runner phases that are still mostly
//! procedural in `agent::harness::subagent_runner`. Runtime cutover can replace
//! each no-op node with the existing effects one phase at a time while keeping a
//! stable topology export for diagnostics.

use tinyagents::graph::export::GraphTopology;
use tinyagents::graph::{
    ClosureStateReducer, CompiledGraph, GraphBuilder, NodeContext, NodeResult,
};

#[derive(Clone, Default)]
pub(crate) struct SubagentPipelineState;

pub(crate) enum SubagentPipelineUpdate {
    Noop,
}

fn noop_node(
    _state: SubagentPipelineState,
    _ctx: NodeContext,
) -> impl std::future::Future<Output = tinyagents::Result<NodeResult<SubagentPipelineUpdate>>> + Send
{
    async move { Ok(NodeResult::Update(SubagentPipelineUpdate::Noop)) }
}

/// Build the fixed sub-agent pipeline graph.
///
/// The node names intentionally match `docs/tinyagents-full-migration-plan/
/// 07-subagents/01-subagent-pipeline.md`:
///
/// `resolve_definition -> prepare_context -> assemble_prompt -> expose_tools ->
/// run_child -> finalize`
pub(crate) fn build_subagent_pipeline_graph(
) -> Result<CompiledGraph<SubagentPipelineState, SubagentPipelineUpdate>, String> {
    GraphBuilder::<SubagentPipelineState, SubagentPipelineUpdate>::new()
        .set_reducer(ClosureStateReducer::new(
            |state: SubagentPipelineState, _update: SubagentPipelineUpdate| Ok(state),
        ))
        .add_node("resolve_definition", noop_node)
        .add_node("prepare_context", noop_node)
        .add_node("assemble_prompt", noop_node)
        .add_node("expose_tools", noop_node)
        .add_node("run_child", noop_node)
        .add_node("finalize", noop_node)
        .add_edge("resolve_definition", "prepare_context")
        .add_edge("prepare_context", "assemble_prompt")
        .add_edge("assemble_prompt", "expose_tools")
        .add_edge("expose_tools", "run_child")
        .add_edge("run_child", "finalize")
        .set_entry("resolve_definition")
        .set_finish("finalize")
        .compile()
        .map_err(|e| format!("sub-agent pipeline graph compile failed: {e}"))
}

/// Structure-only topology of the sub-agent pipeline graph for debug export.
pub(crate) fn subagent_pipeline_topology() -> Result<GraphTopology, String> {
    Ok(build_subagent_pipeline_graph()?.topology())
}
