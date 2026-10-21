//! Declares OpenHuman's product orchestration surfaces over TinyAgents.
//!
//! Tools, by role:
//! - **Spawn**: `spawn_subagent`, `spawn_async_subagent`,
//!   `spawn_parallel_agents`.
//! - **Control**: `steer_subagent`, `continue_subagent`, `close_subagent`,
//!   `wait_subagent`, `wait` / `wait_loop`, `list_subagents`.
//! - **Delegation**: `DelegateGraphTool`, `ArchetypeDelegationTool`,
//!   and `CollapsedDelegationTool` (`delegate_to`).
//!
//! TinyAgents owns generic control operations; the host control module keeps
//! the product declarations and projections. Worker-thread persistence lives
//! in `threads`.
//!
//! `agent::registration` assembles the delegation tools for the catalogue.
//! Execution crosses the shared TinyAgents Jobs/Message and subagent driver
//! ports while this module retains product-specific policies and envelopes.

#[path = "tools/archetype_delegation_host.rs"]
mod archetype_delegation;
#[path = "tools/awaiting_user.rs"]
pub(crate) mod awaiting_user;
#[path = "tools/collapsed_delegation_host.rs"]
mod collapsed_delegation;
#[path = "tools/continue_subagent_host.rs"]
mod continue_subagent;
#[path = "tools/delegate_graph.rs"]
mod delegate_graph;
#[path = "tools/subagent_dispatch_host.rs"]
mod dispatch;
#[path = "tools/spawn_async_subagent_surface.rs"]
mod spawn_async_subagent;
#[path = "tools/spawn_parallel_agents.rs"]
mod spawn_parallel_agents;
#[path = "tools/spawn_subagent_surface.rs"]
mod spawn_subagent;
#[path = "tools/structured_payload.rs"]
pub(crate) mod structured_payload;
#[path = "tools/subagent_abort_report.rs"]
mod subagent_abort_report;
#[path = "tools/subagent_control_surfaces.rs"]
mod subagent_control_surfaces;
#[cfg(test)]
#[path = "tools/tools_e2e_tests.rs"]
mod tools_e2e_tests;

pub(crate) use dispatch::DelegationDispatch;
pub(crate) use subagent_control_surfaces::overlay_task_store_status;
pub(crate) use subagent_control_surfaces::{
    format_running_wait_message, requested_timeout_secs, wait_status_payload,
};
pub(crate) use subagent_control_surfaces::{
    CloseSubagentTool, ListSubagentsTool, SteerSubagentTool, WaitSubagentTool,
};

/// A minimal live TinyAgents carrier over `parent`, for tests that drive a
/// concrete tool directly (outside an agent turn) under an explicit parent
/// execution context. Normal agent turns always arrive through the typed
/// dispatchers with their original carrier.
#[cfg(test)]
pub(crate) fn live_parent_run_context(
    kind: &str,
    parent: crate::agent::harness::ParentExecutionContext,
) -> tinyagents_harness::context::RunContext<crate::agent::tinyagents::host::OpenHumanRunContext> {
    crate::agent::tinyagents::host::OpenHumanRunContext::new()
        .with_host_env(&parent.host_env)
        .with_parent(parent)
        .into_tinyagents(tinyagents_harness::context::RunConfig::new(kind))
}

pub use archetype_delegation::{ArchetypeDelegationTool, DelegationTarget};
pub use collapsed_delegation::{CollapsedDelegationTool, DelegateTarget, DELEGATE_TO_TOOL_NAME};
pub(crate) use continue_subagent::ContinueSubagentDispatch;
pub use continue_subagent::ContinueSubagentTool;
pub(crate) use delegate_graph::DelegateGraphDispatch;
pub use delegate_graph::DelegateGraphTool;
pub(crate) use spawn_async_subagent::SpawnAsyncSubagentDispatch;
pub use spawn_async_subagent::{scope_spawn_async_subagent_spec, SpawnAsyncSubagentTool};
pub(crate) use spawn_parallel_agents::SpawnParallelAgentsDispatch;
pub use spawn_parallel_agents::SpawnParallelAgentsTool;
pub(crate) use spawn_subagent::SpawnSubagentDispatch;
pub use spawn_subagent::SpawnSubagentTool;
