//! Staging admitted tasks into live [`SpawnParallelWorker`] values: worktree
//! preflight for isolated workers, lineage stamping, and the
//! `subagent_spawned` progress/event projection for the `dispatch` phase.

use std::collections::HashMap;
use std::path::Path;

use tinyagents_harness::workspace::{WorkspaceDescriptor, WorkspaceIsolation};
use tokio::sync::mpsc::Sender;

use crate::agent::harness::definition::{AgentDefinition, SandboxMode};
use crate::agent::harness::fork_context::ParentExecutionContext;
use crate::agent::orchestration::worktree;
use crate::agent::progress::AgentProgress;

use super::request::ParallelAgentTask;
use super::staging::{
    prepare_spawn_parallel_tasks_from_defs, worktree_request_for_task, ParallelTaskRejectionKind,
    ParallelWorktreeRequest, SpawnParallelTaskPreflight, WorkerDispatchMode,
};
use super::types::{ParallelAgentLineage, ParallelAgentResult, SpawnParallelWorker};

pub(crate) fn spawn_parallel_lineage(
    parent_session: &str,
    session_parent_prefix: Option<&str>,
    task_id: &str,
) -> ParallelAgentLineage {
    let root_session = session_parent_prefix
        .and_then(|prefix| prefix.split("__").next())
        .filter(|root| !root.is_empty())
        .unwrap_or(parent_session);
    ParallelAgentLineage {
        parent_session: parent_session.to_string(),
        root_session: root_session.to_string(),
        child_task_id: task_id.to_string(),
    }
}

async fn create_spawn_parallel_worktree(
    parent_session: &str,
    action_root: Option<&Path>,
    task_id: &str,
    definition: &AgentDefinition,
    task: &ParallelAgentTask,
    session_parent_prefix: Option<&str>,
) -> Result<Option<WorkspaceDescriptor>, ParallelAgentResult> {
    match worktree_request_for_task(task) {
        ParallelWorktreeRequest::SharedWorkspace => Ok(None),
        ParallelWorktreeRequest::Isolated { base_ref } => match action_root {
            Some(repo_root) => {
                let sandbox = match definition.sandbox_mode {
                    SandboxMode::Sandboxed => tinyagents_harness::tool::SandboxMode::Required,
                    SandboxMode::None | SandboxMode::ReadOnly => {
                        tinyagents_harness::tool::SandboxMode::Inherit
                    }
                };
                let isolation = worktree::OpenHumanWorktreeIsolation::new(repo_root)
                    .with_base_ref(base_ref)
                    .with_sandbox(sandbox);
                match isolation.prepare(task_id, Some(&definition.id)).await {
                    Ok(descriptor) => {
                        tracing::debug!(
                            parent_session = %parent_session,
                            task_id = %task_id,
                            worktree = %descriptor.root.display(),
                            policy_id = %descriptor.policy_id,
                            base_ref = base_ref.as_str(),
                            "[spawn_parallel_agents] prepared isolated workspace descriptor"
                        );
                        Ok(Some(descriptor))
                    }
                    Err(err) => {
                        tracing::warn!(
                            parent_session = %parent_session,
                            task_id = %task_id,
                            error = %err,
                            "[spawn_parallel_agents] workspace_prepare_failed"
                        );
                        Err(ParallelAgentResult {
                            task_id: task_id.to_string(),
                            agent_id: definition.id.clone(),
                            lineage: spawn_parallel_lineage(
                                parent_session,
                                session_parent_prefix,
                                task_id,
                            ),
                            success: false,
                            output: None,
                            error: Some(format!("worktree isolation failed: {err}")),
                            ownership: task.ownership.clone(),
                            elapsed_ms: 0,
                            iterations: 0,
                            stale_parent_reads: Vec::new(),
                            worktree_path: None,
                            changed_files: Vec::new(),
                            dirty_status: None,
                        })
                    }
                }
            }
            None => {
                tracing::warn!(
                    parent_session = %parent_session,
                    task_id = %task_id,
                    "[spawn_parallel_agents] worktree_requested_but_no_action_dir"
                );
                Err(ParallelAgentResult {
                    task_id: task_id.to_string(),
                    agent_id: definition.id.clone(),
                    lineage: spawn_parallel_lineage(parent_session, session_parent_prefix, task_id),
                    success: false,
                    output: None,
                    error: Some(
                        "worktree isolation requested but action_dir is unavailable".to_string(),
                    ),
                    ownership: task.ownership.clone(),
                    elapsed_ms: 0,
                    iterations: 0,
                    stale_parent_reads: Vec::new(),
                    worktree_path: None,
                    changed_files: Vec::new(),
                    dirty_status: None,
                })
            }
        },
    }
}

pub(crate) async fn stage_spawn_parallel_workers_from_defs(
    parent_session: &str,
    progress_sink: Option<&Sender<AgentProgress>>,
    tasks: Vec<ParallelAgentTask>,
    definitions: &HashMap<String, AgentDefinition>,
    parent: &ParentExecutionContext,
    action_root: Option<&Path>,
    parent_workspace_descriptor: Option<&WorkspaceDescriptor>,
) -> (Vec<SpawnParallelWorker>, Vec<ParallelAgentResult>) {
    let mut immediate_results = Vec::new();
    let mut prepared = Vec::new();

    for preflight in prepare_spawn_parallel_tasks_from_defs(tasks, definitions, parent) {
        let (definition, prompt, task, task_id, dispatch_mode) = match preflight {
            SpawnParallelTaskPreflight::Rejected(rejection) => {
                match rejection.kind {
                    ParallelTaskRejectionKind::MissingAgentOrPrompt => {
                        tracing::debug!(
                            parent_session = %parent_session,
                            task_id = %rejection.task_id,
                            agent_id = %rejection.agent_id,
                            "[spawn_parallel_agents] invalid_task_missing_agent_or_prompt"
                        );
                    }
                    ParallelTaskRejectionKind::UnknownAgent => {
                        tracing::debug!(
                            parent_session = %parent_session,
                            task_id = %rejection.task_id,
                            agent_id = %rejection.agent_id,
                            "[spawn_parallel_agents] invalid_task_unknown_agent"
                        );
                    }
                    ParallelTaskRejectionKind::OutsideAllowlist => {
                        tracing::warn!(
                            parent_session = %parent_session,
                            parent_agent = %parent.agent_definition_id,
                            task_id = %rejection.task_id,
                            agent_id = %rejection.agent_id,
                            allowed = ?parent.allowed_subagent_ids,
                            "[spawn_parallel_agents] rejected_task_outside_subagent_allowlist"
                        );
                    }
                    ParallelTaskRejectionKind::MissingToolkit => {
                        tracing::debug!(
                            parent_session = %parent_session,
                            task_id = %rejection.task_id,
                            agent_id = %rejection.agent_id,
                            "[spawn_parallel_agents] invalid_task_missing_toolkit"
                        );
                    }
                    ParallelTaskRejectionKind::RequiresIsolation => {
                        tracing::warn!(
                            parent_session = %parent_session,
                            task_id = %rejection.task_id,
                            agent_id = %rejection.agent_id,
                            ownership = rejection.ownership.as_deref().unwrap_or(""),
                            "[spawn_parallel_agents] rejected_shared_workspace_write_capable_task"
                        );
                    }
                }
                let lineage = spawn_parallel_lineage(
                    parent_session,
                    parent.session_parent_prefix.as_deref(),
                    &rejection.task_id,
                );
                immediate_results.push(ParallelAgentResult {
                    task_id: rejection.task_id,
                    agent_id: rejection.agent_id,
                    lineage,
                    success: false,
                    output: None,
                    error: Some(rejection.error),
                    ownership: rejection.ownership,
                    elapsed_ms: 0,
                    iterations: 0,
                    stale_parent_reads: Vec::new(),
                    worktree_path: None,
                    changed_files: Vec::new(),
                    dirty_status: None,
                });
                continue;
            }
            SpawnParallelTaskPreflight::Prepared(prepared_task) => (
                prepared_task.definition,
                prepared_task.prompt,
                prepared_task.task,
                prepared_task.task_id,
                prepared_task.dispatch_mode,
            ),
        };
        project_spawn_parallel_spawned(
            parent_session,
            progress_sink,
            &definition,
            &task_id,
            &prompt,
            task.ownership
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some(),
        )
        .await;
        let workspace_descriptor = match create_spawn_parallel_worktree(
            parent_session,
            action_root,
            &task_id,
            &definition,
            &task,
            parent.session_parent_prefix.as_deref(),
        )
        .await
        {
            Ok(descriptor) => descriptor,
            Err(result) => {
                immediate_results.push(result);
                continue;
            }
        };
        let worktree_path = workspace_descriptor
            .as_ref()
            .map(|descriptor| descriptor.root.clone());
        let worker_workspace_descriptor = workspace_descriptor
            .clone()
            .or_else(|| parent_workspace_descriptor.cloned());
        let lineage = spawn_parallel_lineage(
            parent_session,
            parent.session_parent_prefix.as_deref(),
            &task_id,
        );
        prepared.push(SpawnParallelWorker {
            definition,
            prompt,
            task,
            task_id,
            lineage,
            worktree_path,
            workspace_descriptor: worker_workspace_descriptor,
            dispatch_mode,
        });
    }

    tracing::debug!(
        parent_session = %parent_session,
        prepared_count = prepared.len(),
        immediate_count = immediate_results.len(),
        serial_write_count = prepared
            .iter()
            .filter(|worker| matches!(
                worker.dispatch_mode,
                WorkerDispatchMode::SerialSharedWorkspaceWrite
            ))
            .count(),
        "[spawn_parallel_agents] prepared_tasks"
    );
    (prepared, immediate_results)
}

async fn project_spawn_parallel_spawned(
    parent_session: &str,
    progress_sink: Option<&Sender<AgentProgress>>,
    definition: &AgentDefinition,
    task_id: &str,
    prompt: &str,
    has_ownership: bool,
) {
    let prompt_chars = prompt.chars().count();
    tracing::debug!(
        parent_session = %parent_session,
        task_id = %task_id,
        agent_id = %definition.id,
        prompt_chars,
        has_ownership,
        "[spawn_parallel_agents] publishing_subagent_spawned"
    );
    crate::agent::orchestration::subagent_events::publish_subagent_spawned(
        parent_session.to_string(),
        definition.id.clone(),
        "typed".to_string(),
        task_id.to_string(),
        prompt_chars,
    );
    if let Some(tx) = progress_sink {
        if let Err(err) = tx
            .send(AgentProgress::SubagentSpawned {
                agent_id: definition.id.clone(),
                task_id: task_id.to_string(),
                mode: "typed".to_string(),
                dedicated_thread: false,
                prompt_chars,
                prompt: prompt.to_string(),
                worker_thread_id: None,
                display_name: Some(definition.display_name().to_string()),
            })
            .await
        {
            tracing::debug!(
                parent_session = %parent_session,
                task_id = %task_id,
                agent_id = %definition.id,
                error = %err,
                "[spawn_parallel_agents] progress_send_failed spawned"
            );
        }
    }
}
