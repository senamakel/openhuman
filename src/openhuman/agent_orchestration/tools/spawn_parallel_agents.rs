//! Tool: `spawn_parallel_agents` — fan out independent sub-agent tasks.

use crate::core::event_bus::{DomainEvent, publish_global};
use crate::openhuman::agent::harness::definition::AgentDefinitionRegistry;
use crate::openhuman::agent::harness::fork_context::current_parent;
use crate::openhuman::agent::progress::AgentProgress;
#[cfg(test)]
use crate::openhuman::agent_orchestration::spawn_parallel_graph::ParallelAgentTask;
#[cfg(test)]
use crate::openhuman::agent_orchestration::spawn_parallel_graph::with_ownership_boundary;
use crate::openhuman::agent_orchestration::spawn_parallel_graph::{
    ParallelAgentResult, ParallelTaskRejectionKind, ParallelWorktreeRequest,
    SpawnParallelTaskPreflight, SpawnParallelWorker, collect_spawn_parallel_results,
    format_spawn_parallel_success, prepare_spawn_parallel_tasks, run_spawn_parallel_workers,
    validate_spawn_parallel_tasks, worktree_request_for_task,
};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct SpawnParallelAgentsTool;

impl SpawnParallelAgentsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpawnParallelAgentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SpawnParallelAgentsTool {
    fn name(&self) -> &str {
        "spawn_parallel_agents"
    }

    fn description(&self) -> &str {
        "Run two or more independent sub-agent tasks concurrently and collect their results. \
         Use only when tasks have clear non-overlapping ownership or read-only scopes. Each task \
         has `{agent_id, prompt, context?, toolkit?, ownership?}`; include `ownership` for file, \
         module, or responsibility boundaries so workers do not overlap."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let agent_ids: Vec<String> = AgentDefinitionRegistry::global()
            .map(|reg| reg.list().iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default();
        let agent_id_schema = if agent_ids.is_empty() {
            json!({ "type": "string" })
        } else {
            json!({ "type": "string", "enum": agent_ids })
        };
        json!({
            "type": "object",
            "required": ["tasks"],
            "properties": {
                "tasks": {
                    "type": "array",
                    "minItems": 2,
                    "items": {
                        "type": "object",
                        "required": ["agent_id", "prompt"],
                        "properties": {
                            "agent_id": agent_id_schema,
                            "prompt": { "type": "string" },
                            "context": { "type": "string" },
                            "toolkit": { "type": "string" },
                            "ownership": {
                                "type": "string",
                                "description": "Disjoint file/module/responsibility boundary for this worker."
                            },
                            "isolation": {
                                "type": "string",
                                "enum": ["none", "worktree"],
                                "description": "File-isolation strategy. `none` (default) shares the workspace; `worktree` gives this edit-capable worker its own git worktree checkout so parallel edits never collide. Use `worktree` only for edit-capable coding workers, not read-only ones."
                            },
                            "base_ref": {
                                "type": "string",
                                "enum": ["head", "fresh"],
                                "description": "For `isolation = worktree`: branch the worktree from current HEAD (`head`, default) or the repo's default branch (`fresh`)."
                            }
                        }
                    }
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tracing::debug!("[spawn_parallel_agents] execute entry");
        let tasks = match validate_spawn_parallel_tasks(&args, None) {
            Ok(tasks) => tasks,
            Err(message) if message == "Missing 'tasks' parameter" => {
                tracing::debug!("[spawn_parallel_agents] missing_tasks_parameter");
                return Err(anyhow::anyhow!(message));
            }
            Err(message) if message.starts_with("Invalid tasks array:") => {
                tracing::debug!(error = %message, "[spawn_parallel_agents] invalid_tasks_array");
                return Err(anyhow::anyhow!(message));
            }
            Err(message) => {
                tracing::debug!("[spawn_parallel_agents] rejected_too_few_tasks");
                return Ok(ToolResult::error(message));
            }
        };

        let parent = match current_parent() {
            Some(parent) => parent,
            None => {
                tracing::debug!("[spawn_parallel_agents] rejected_outside_agent_turn");
                return Ok(ToolResult::error(
                    "spawn_parallel_agents called outside of an agent turn",
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
        let tasks = match validate_spawn_parallel_tasks(&args, Some(max_parallel)) {
            Ok(tasks) => tasks,
            Err(message) => {
                tracing::debug!(
                    parent_session = %parent.session_id,
                    task_count = tasks.len(),
                    max_parallel,
                    "[spawn_parallel_agents] rejected_too_many_tasks"
                );
                return Ok(ToolResult::error(message));
            }
        };

        let registry = match AgentDefinitionRegistry::global() {
            Some(registry) => registry,
            None => {
                tracing::debug!("[spawn_parallel_agents] registry_unavailable");
                return Ok(ToolResult::error(
                    "spawn_parallel_agents: AgentDefinitionRegistry has not been initialised",
                ));
            }
        };

        let parent_session = parent.session_id.clone();
        let progress_sink = parent.on_progress.clone();
        let mut immediate_results = Vec::new();
        let mut prepared = Vec::new();

        // Resolve the agent sandbox root once — used as the repo root when a
        // task opts into git-worktree isolation. This is `Config.action_dir`
        // (the user's project repo the coding agent edits), NOT openhuman's
        // own tree. Loaded lazily; only consulted for worktree-isolated tasks.
        let action_root: Option<std::path::PathBuf> =
            crate::openhuman::config::Config::load_or_init()
                .await
                .ok()
                .map(|cfg| cfg.action_dir.clone());

        for preflight in prepare_spawn_parallel_tasks(tasks, registry, &parent) {
            let (definition, prompt, task, task_id) = match preflight {
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
                    }
                    immediate_results.push(ParallelAgentResult {
                        task_id: rejection.task_id,
                        agent_id: rejection.agent_id,
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
                ),
            };
            tracing::debug!(
                parent_session = %parent_session,
                task_id = %task_id,
                agent_id = %definition.id,
                prompt_chars = prompt.chars().count(),
                has_ownership = task.ownership.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_some(),
                "[spawn_parallel_agents] publishing_subagent_spawned"
            );
            publish_global(DomainEvent::SubagentSpawned {
                parent_session: parent_session.clone(),
                agent_id: definition.id.clone(),
                mode: "typed".to_string(),
                task_id: task_id.clone(),
                prompt_chars: prompt.chars().count(),
            });
            if let Some(ref tx) = progress_sink {
                if let Err(err) = tx
                    .send(AgentProgress::SubagentSpawned {
                        agent_id: definition.id.clone(),
                        task_id: task_id.clone(),
                        mode: "typed".to_string(),
                        dedicated_thread: false,
                        prompt_chars: prompt.chars().count(),
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
            // ── Optional git-worktree isolation ────────────────────────────
            // When the task requests `isolation = "worktree"`, create a
            // dedicated worktree under the user's project repo and run this
            // worker with its `action_dir` pointed there. On any failure we
            // surface an immediate error result rather than silently falling
            // back to the shared workspace (which is the exact collision this
            // feature prevents).
            let worktree_path = match worktree_request_for_task(&task) {
                ParallelWorktreeRequest::Isolated { base_ref } => {
                    use crate::openhuman::agent_orchestration::worktree;
                    match action_root.as_ref() {
                        Some(repo_root) => match worktree::create(repo_root, &task_id, base_ref) {
                            Ok(status) => {
                                tracing::debug!(
                                    parent_session = %parent_session,
                                    task_id = %task_id,
                                    worktree = %status.path.display(),
                                    base_ref = base_ref.as_str(),
                                    "[spawn_parallel_agents] created isolated worktree"
                                );
                                Some(status.path)
                            }
                            Err(err) => {
                                tracing::warn!(
                                    parent_session = %parent_session,
                                    task_id = %task_id,
                                    error = %err,
                                    "[spawn_parallel_agents] worktree_create_failed"
                                );
                                immediate_results.push(ParallelAgentResult {
                                    task_id,
                                    agent_id: definition.id.clone(),
                                    success: false,
                                    output: None,
                                    error: Some(format!("worktree isolation failed: {err}")),
                                    ownership: task.ownership,
                                    elapsed_ms: 0,
                                    iterations: 0,
                                    stale_parent_reads: Vec::new(),
                                    worktree_path: None,
                                    changed_files: Vec::new(),
                                    dirty_status: None,
                                });
                                continue;
                            }
                        },
                        None => {
                            tracing::warn!(
                                parent_session = %parent_session,
                                task_id = %task_id,
                                "[spawn_parallel_agents] worktree_requested_but_no_action_dir"
                            );
                            immediate_results.push(ParallelAgentResult {
                                task_id,
                                agent_id: definition.id.clone(),
                                success: false,
                                output: None,
                                error: Some(
                                    "worktree isolation requested but action_dir is unavailable"
                                        .to_string(),
                                ),
                                ownership: task.ownership,
                                elapsed_ms: 0,
                                iterations: 0,
                                stale_parent_reads: Vec::new(),
                                worktree_path: None,
                                changed_files: Vec::new(),
                                dirty_status: None,
                            });
                            continue;
                        }
                    }
                }
                ParallelWorktreeRequest::SharedWorkspace => None,
            };
            prepared.push(SpawnParallelWorker {
                definition,
                prompt,
                task,
                task_id,
                worktree_path,
            });
        }
        tracing::debug!(
            parent_session = %parent_session,
            prepared_count = prepared.len(),
            immediate_count = immediate_results.len(),
            "[spawn_parallel_agents] prepared_tasks"
        );

        // Fan the prepared workers out through the spawn graph module. Results
        // come back in prepared order, then we append them after immediate
        // pre-flight failures — the existing compatibility ordering.
        let fanned = run_spawn_parallel_workers(prepared, action_root.clone())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let mut results = immediate_results;
        for result in fanned {
            match &result {
                ParallelAgentResult {
                    success: true,
                    agent_id,
                    task_id,
                    elapsed_ms,
                    iterations,
                    output,
                    worktree_path,
                    changed_files,
                    dirty_status,
                    ..
                } => {
                    tracing::debug!(
                        parent_session = %parent_session,
                        task_id = %task_id,
                        agent_id = %agent_id,
                        elapsed_ms = *elapsed_ms,
                        iterations = *iterations,
                        "[spawn_parallel_agents] publishing_subagent_completed"
                    );
                    publish_global(DomainEvent::SubagentCompleted {
                        parent_session: parent_session.clone(),
                        task_id: task_id.clone(),
                        agent_id: agent_id.clone(),
                        elapsed_ms: *elapsed_ms,
                        output_chars: output.as_ref().map(|s| s.chars().count()).unwrap_or(0),
                        iterations: *iterations as usize,
                    });
                    if let Some(ref tx) = progress_sink {
                        if let Err(err) = tx
                            .send(AgentProgress::SubagentCompleted {
                                agent_id: agent_id.clone(),
                                task_id: task_id.clone(),
                                elapsed_ms: *elapsed_ms,
                                iterations: *iterations,
                                output_chars: output
                                    .as_ref()
                                    .map(|s| s.chars().count())
                                    .unwrap_or(0),
                                worktree_path: worktree_path.clone(),
                                changed_files: changed_files.clone(),
                                dirty_status: *dirty_status,
                            })
                            .await
                        {
                            tracing::debug!(
                                parent_session = %parent_session,
                                task_id = %task_id,
                                agent_id = %agent_id,
                                error = %err,
                                "[spawn_parallel_agents] progress_send_failed completed"
                            );
                        }
                    }
                }
                ParallelAgentResult {
                    success: false,
                    agent_id,
                    task_id,
                    error,
                    ..
                } => {
                    let message = error
                        .clone()
                        .unwrap_or_else(|| "unknown failure".to_string());
                    tracing::debug!(
                        parent_session = %parent_session,
                        task_id = %task_id,
                        agent_id = %agent_id,
                        error = %message,
                        "[spawn_parallel_agents] publishing_subagent_failed"
                    );
                    publish_global(DomainEvent::SubagentFailed {
                        parent_session: parent_session.clone(),
                        task_id: task_id.clone(),
                        agent_id: agent_id.clone(),
                        error: message.clone(),
                    });
                    if let Some(ref tx) = progress_sink {
                        if let Err(err) = tx
                            .send(AgentProgress::SubagentFailed {
                                agent_id: agent_id.clone(),
                                task_id: task_id.clone(),
                                error: message,
                            })
                            .await
                        {
                            tracing::debug!(
                                parent_session = %parent_session,
                                task_id = %task_id,
                                agent_id = %agent_id,
                                error = %err,
                                "[spawn_parallel_agents] progress_send_failed failed"
                            );
                        }
                    }
                }
            }
            results.push(result);
        }

        let collected = collect_spawn_parallel_results(&parent_session, results);
        tracing::debug!(
            parent_session = %parent_session,
            total = collected.total(),
            succeeded = collected.succeeded(),
            failed = collected.failures,
            overlaps = collected.overlap_warnings.len(),
            "[spawn_parallel_agents] execute exit"
        );
        Ok(ToolResult::success(format_spawn_parallel_success(
            &collected,
        )))
    }
}

#[cfg(test)]
#[path = "spawn_parallel_agents_tests.rs"]
mod tests;
