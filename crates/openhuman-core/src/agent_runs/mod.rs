//! Product-facing RPC surfaces for agent runs, teams, workflows, replay, and tools.

pub mod agent_chat;
pub(crate) mod agent_teams;
pub(crate) mod command_center;
pub(crate) mod context_breakdown;
pub(crate) mod replay;
mod schemas;
pub(crate) mod subagent_control;
mod subagent_finalize;
pub(crate) mod workflow_runs;
pub(crate) mod worktree_schemas;

pub(crate) use agent_chat::all_inference_agent_chat_registered_controllers;
pub(crate) use agent_teams::{
    all_agent_team_controller_schemas, all_agent_team_registered_controllers,
};
/// The canonical RPC method name for `inference.agent_chat`.
///
/// The controller's `namespace` + `function` combine into the wire method
/// `openhuman.inference_agent_chat` ([`rpc_method_name`](crate::core::ControllerSchema)).
/// Host facades (the embed library) reference this constant rather than
/// spelling the string out, so a rename upstream cannot silently drift an
/// embedder's dispatch string away from the registered controller.
pub const INFERENCE_AGENT_CHAT: &str = "openhuman.inference_agent_chat";
pub(crate) use command_center::{
    all_command_center_controller_schemas, all_command_center_registered_controllers,
};
pub(crate) use schemas::all_registered_controllers as all_agent_registered_controllers;
pub(crate) use subagent_control::{
    all_controller_schemas as all_subagent_control_controller_schemas,
    all_registered_controllers as all_subagent_control_registered_controllers,
};
pub(crate) use workflow_runs::{
    all_workflow_run_controller_schemas, all_workflow_run_registered_controllers,
};
pub(crate) use worktree_schemas::{
    all_controller_schemas as all_worktree_controller_schemas,
    all_registered_controllers as all_worktree_registered_controllers,
};

/// Registers the agent-run lifecycle projection for detached sub-agent events.
pub(crate) fn register_subagent_finalize_subscriber(config: &crate::config::Config) {
    subagent_finalize::register_subagent_finalize_subscriber(config);
}

/// Registers the orchestration RPC surfaces in their catalogue order.
pub(crate) fn all_orchestration_registered_controllers(
) -> Vec<crate::core::all::RegisteredController> {
    let mut controllers = Vec::new();
    controllers.extend(all_command_center_registered_controllers());
    controllers.extend(all_workflow_run_registered_controllers());
    controllers.extend(all_agent_team_registered_controllers());
    controllers.extend(all_worktree_registered_controllers());
    controllers.extend(all_subagent_control_registered_controllers());
    controllers
}

/// Returns orchestration schemas in the same order as their registered controllers.
pub(crate) fn all_orchestration_controller_schemas() -> Vec<crate::core::ControllerSchema> {
    let mut schemas = Vec::new();
    schemas.extend(all_command_center_controller_schemas());
    schemas.extend(all_workflow_run_controller_schemas());
    schemas.extend(all_agent_team_controller_schemas());
    schemas.extend(all_worktree_controller_schemas());
    schemas.extend(all_subagent_control_controller_schemas());
    schemas
}
