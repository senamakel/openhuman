//! Public entry points for running the `spawn_parallel_agents` graph:
//! resolving the parent turn context and agent registry, then handing off to
//! [`run_spawn_parallel_execution_graph`](super::graph::run_spawn_parallel_execution_graph).

use std::path::PathBuf;

use tinyagents_harness::workspace::WorkspaceDescriptor;
use tinyagents_harness::CancellationToken;

use crate::agent::harness::definition::AgentDefinitionRegistry;
use crate::agent::harness::fork_context::current_parent;

use super::collect::SpawnParallelGraphOutcome;
use super::graph::run_spawn_parallel_execution_graph;
use super::request::validate_spawn_parallel_tool_request;
use super::staging::snapshot_agent_definitions;

pub(crate) async fn run_spawn_parallel_graph(
    args: serde_json::Value,
) -> Result<SpawnParallelGraphOutcome, String> {
    run_spawn_parallel_graph_with_workspace(args, None).await
}

pub(crate) async fn run_spawn_parallel_graph_with_workspace(
    args: serde_json::Value,
    parent_workspace_descriptor: Option<WorkspaceDescriptor>,
) -> Result<SpawnParallelGraphOutcome, String> {
    run_spawn_parallel_graph_with_cancellation_and_workspace(
        args,
        CancellationToken::new(),
        parent_workspace_descriptor,
    )
    .await
}

pub(crate) async fn run_spawn_parallel_graph_with_cancellation(
    args: serde_json::Value,
    cancel: CancellationToken,
) -> Result<SpawnParallelGraphOutcome, String> {
    run_spawn_parallel_graph_with_cancellation_and_workspace(args, cancel, None).await
}

pub(crate) async fn run_spawn_parallel_graph_with_cancellation_and_workspace(
    args: serde_json::Value,
    cancel: CancellationToken,
    parent_workspace_descriptor: Option<WorkspaceDescriptor>,
) -> Result<SpawnParallelGraphOutcome, String> {
    let tasks = match validate_spawn_parallel_tool_request(&args, None) {
        Ok(tasks) => tasks,
        Err(err) => return Ok(SpawnParallelGraphOutcome::InvalidRequest(err)),
    };

    let parent = match current_parent() {
        Some(parent) => parent,
        None => {
            tracing::debug!("[spawn_parallel_agents] rejected_outside_agent_turn");
            return Ok(SpawnParallelGraphOutcome::Rejected(
                "spawn_parallel_agents called outside of an agent turn".to_string(),
            ));
        }
    };
    let max_parallel = parent.agent_config.max_parallel_tools.max(2);
    tracing::debug!(
        parent_session = %parent.session_id,
        task_count = tasks.len(),
        max_parallel,
        "[spawn_parallel_agents] validated_parent_context"
    );
    let registry = match AgentDefinitionRegistry::global() {
        Some(registry) => registry,
        None => {
            tracing::debug!("[spawn_parallel_agents] registry_unavailable");
            return Ok(SpawnParallelGraphOutcome::Rejected(
                "spawn_parallel_agents: AgentDefinitionRegistry has not been initialised"
                    .to_string(),
            ));
        }
    };

    let parent_session = parent.session_id.clone();
    let progress_sink = parent.on_progress.clone();
    let action_root =
        resolve_spawn_parallel_action_root(parent_workspace_descriptor.as_ref()).await;
    let definitions = snapshot_agent_definitions(registry);
    let outcome = run_spawn_parallel_execution_graph(
        &parent_session,
        progress_sink,
        tasks,
        max_parallel,
        definitions,
        parent,
        action_root,
        cancel,
        parent_workspace_descriptor,
    )
    .await?;
    match &outcome {
        SpawnParallelGraphOutcome::Collected(collected) => {
            tracing::debug!(
                parent_session = %parent_session,
                total = collected.total(),
                succeeded = collected.succeeded(),
                failed = collected.failures,
                overlaps = collected.overlap_warnings.len(),
                "[spawn_parallel_agents] execute exit"
            );
        }
        SpawnParallelGraphOutcome::Rejected(message) => {
            tracing::debug!(
                parent_session = %parent_session,
                error = %message,
                "[spawn_parallel_agents] rejected_by_graph_validate"
            );
        }
        SpawnParallelGraphOutcome::InvalidRequest(_) => {
            tracing::debug!(
                parent_session = %parent_session,
                "[spawn_parallel_agents] invalid_request_after_graph_run"
            );
        }
        SpawnParallelGraphOutcome::Cancelled(message) => {
            tracing::debug!(
                parent_session = %parent_session,
                message = %message,
                "[spawn_parallel_agents] cancelled_by_graph"
            );
        }
    }
    Ok(outcome)
}

/// Resolve the agent sandbox root once for the graph run.
///
/// This is `Config.action_dir` (the user's project repo the coding agent edits),
/// NOT OpenHuman's own tree. It is only consulted when a worker asks for
/// git-worktree isolation; failures preserve the previous `None` fallback.
async fn resolve_spawn_parallel_action_root(
    parent_workspace_descriptor: Option<&WorkspaceDescriptor>,
) -> Option<PathBuf> {
    if let Some(descriptor) = parent_workspace_descriptor {
        tracing::debug!(
            action_root = %descriptor.root.display(),
            policy_id = %descriptor.policy_id,
            "[spawn_parallel_agents] using ToolExecutionContext workspace root for graph"
        );
        return Some(descriptor.root.clone());
    }
    match crate::config::Config::load_or_init().await {
        Ok(config) => {
            tracing::debug!(
                action_root = %config.action_dir.display(),
                "[spawn_parallel_agents] resolved action root for graph"
            );
            Some(config.action_dir.clone())
        }
        Err(err) => {
            tracing::debug!(
                error = %err,
                "[spawn_parallel_agents] config load failed; worktree isolation will use missing-root fallback"
            );
            None
        }
    }
}
