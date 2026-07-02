//! Structure-only graph scaffold for `spawn_parallel_agents`.
//!
//! The tool wrapper still owns parent context and `ToolResult` translation.
//! This module owns the graph-side request validation, worktree preflight,
//! progress/event projection, worker fanout, final JSON formatting, and the
//! topology surface from `docs/tinyagents-full-migration-plan/08-orchestration/
//! 02-spawn-parallel-graph.md`.

use tinyagents::graph::export::GraphTopology;
use tinyagents::graph::{
    ClosureStateReducer, CompiledGraph, GraphBuilder, NodeContext, NodeResult,
};

use crate::core::event_bus::{DomainEvent, publish_global};
use crate::openhuman::agent::harness::definition::{AgentDefinition, AgentDefinitionRegistry};
use crate::openhuman::agent::harness::fork_context::ParentExecutionContext;
use crate::openhuman::agent::harness::subagent_runner::{SubagentRunOptions, run_subagent};
use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::agent_orchestration::worktree::{self, BaseRef};
use crate::openhuman::file_state;
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::Sender;

/// One requested worker in a `spawn_parallel_agents` call.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ParallelAgentTask {
    pub(crate) agent_id: String,
    pub(crate) prompt: String,
    #[serde(default)]
    pub(crate) context: Option<String>,
    #[serde(default)]
    pub(crate) toolkit: Option<String>,
    #[serde(default)]
    pub(crate) ownership: Option<String>,
    /// File-isolation strategy for this worker: `"none"` (default) or
    /// `"worktree"` (dedicated git worktree checkout).
    #[serde(default)]
    pub(crate) isolation: Option<String>,
    /// Worktree base ref: `"head"` (default) or `"fresh"`.
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
}

/// Decode and validate the request batch before the live worker fanout.
///
/// This is the first real `validate`-node responsibility moved out of the tool
/// wrapper. Effectful worktree creation remains in the wrapper until the graph
/// carries those dependencies explicitly.
pub(crate) fn validate_spawn_parallel_tasks(
    args: &serde_json::Value,
    max_parallel: Option<usize>,
) -> Result<Vec<ParallelAgentTask>, String> {
    let tasks_value = args
        .get("tasks")
        .cloned()
        .ok_or_else(|| "Missing 'tasks' parameter".to_string())?;
    let tasks: Vec<ParallelAgentTask> =
        serde_json::from_value(tasks_value).map_err(|e| format!("Invalid tasks array: {e}"))?;

    if tasks.len() < 2 {
        return Err("spawn_parallel_agents requires at least two tasks".to_string());
    }
    if let Some(max_parallel) = max_parallel {
        if tasks.len() > max_parallel {
            return Err(format!(
                "spawn_parallel_agents received {} tasks but max_parallel_tools is {}",
                tasks.len(),
                max_parallel
            ));
        }
    }

    Ok(tasks)
}

/// Prepared worker ready for the live dispatch/worker phases.
pub(crate) struct PreparedParallelTask {
    pub(crate) definition: AgentDefinition,
    pub(crate) prompt: String,
    pub(crate) task: ParallelAgentTask,
    pub(crate) task_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelTaskRejectionKind {
    MissingAgentOrPrompt,
    UnknownAgent,
    OutsideAllowlist,
    MissingToolkit,
}

pub(crate) struct ParallelTaskRejection {
    pub(crate) task_id: String,
    pub(crate) agent_id: String,
    pub(crate) error: String,
    pub(crate) ownership: Option<String>,
    pub(crate) kind: ParallelTaskRejectionKind,
}

pub(crate) enum SpawnParallelTaskPreflight {
    Prepared(PreparedParallelTask),
    Rejected(ParallelTaskRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelWorktreeRequest {
    SharedWorkspace,
    Isolated { base_ref: BaseRef },
}

pub(crate) fn worktree_request_for_task(task: &ParallelAgentTask) -> ParallelWorktreeRequest {
    let isolated = task
        .isolation
        .as_deref()
        .map(str::trim)
        .map(|s| s.eq_ignore_ascii_case("worktree"))
        .unwrap_or(false);
    if isolated {
        ParallelWorktreeRequest::Isolated {
            base_ref: BaseRef::parse(task.base_ref.as_deref()),
        }
    } else {
        ParallelWorktreeRequest::SharedWorkspace
    }
}

pub(crate) fn create_spawn_parallel_worktree(
    parent_session: &str,
    action_root: Option<&Path>,
    task_id: &str,
    agent_id: &str,
    task: &ParallelAgentTask,
) -> Result<Option<PathBuf>, ParallelAgentResult> {
    match worktree_request_for_task(task) {
        ParallelWorktreeRequest::SharedWorkspace => Ok(None),
        ParallelWorktreeRequest::Isolated { base_ref } => match action_root {
            Some(repo_root) => match worktree::create(repo_root, task_id, base_ref) {
                Ok(status) => {
                    tracing::debug!(
                        parent_session = %parent_session,
                        task_id = %task_id,
                        worktree = %status.path.display(),
                        base_ref = base_ref.as_str(),
                        "[spawn_parallel_agents] created isolated worktree"
                    );
                    Ok(Some(status.path))
                }
                Err(err) => {
                    tracing::warn!(
                        parent_session = %parent_session,
                        task_id = %task_id,
                        error = %err,
                        "[spawn_parallel_agents] worktree_create_failed"
                    );
                    Err(ParallelAgentResult {
                        task_id: task_id.to_string(),
                        agent_id: agent_id.to_string(),
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
            },
            None => {
                tracing::warn!(
                    parent_session = %parent_session,
                    task_id = %task_id,
                    "[spawn_parallel_agents] worktree_requested_but_no_action_dir"
                );
                Err(ParallelAgentResult {
                    task_id: task_id.to_string(),
                    agent_id: agent_id.to_string(),
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

/// Resolve and validate each worker before the tool performs effectful work.
///
/// This models the graph's validate/dispatch preflight without moving runtime
/// effects out of the current tool wrapper yet.
pub(crate) fn prepare_spawn_parallel_tasks(
    tasks: Vec<ParallelAgentTask>,
    registry: &AgentDefinitionRegistry,
    parent: &ParentExecutionContext,
) -> Vec<SpawnParallelTaskPreflight> {
    tasks
        .into_iter()
        .map(|task| {
            let agent_id = task.agent_id.trim().to_string();
            let prompt = task.prompt.trim().to_string();
            let task_id = format!("sub-{}", uuid::Uuid::new_v4());

            if agent_id.is_empty() || prompt.is_empty() {
                return SpawnParallelTaskPreflight::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id,
                    error: "agent_id and prompt are required".to_string(),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::MissingAgentOrPrompt,
                });
            }

            let Some(definition) = registry.get(&agent_id).cloned() else {
                return SpawnParallelTaskPreflight::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id: agent_id.clone(),
                    error: format!("unknown agent_id '{agent_id}'"),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::UnknownAgent,
                });
            };

            if !parent.allowed_subagent_ids.contains(&definition.id) {
                return SpawnParallelTaskPreflight::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id: definition.id.clone(),
                    error: format!(
                        "agent '{}' is not in parent agent '{}' subagents.allowlist",
                        definition.id, parent.agent_definition_id
                    ),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::OutsideAllowlist,
                });
            }

            if definition.id == "integrations_agent"
                && task
                    .toolkit
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            {
                return SpawnParallelTaskPreflight::Rejected(ParallelTaskRejection {
                    task_id,
                    agent_id,
                    error: "integrations_agent requires toolkit".to_string(),
                    ownership: task.ownership,
                    kind: ParallelTaskRejectionKind::MissingToolkit,
                });
            }

            let prompt = with_ownership_boundary(&prompt, task.ownership.as_deref());
            SpawnParallelTaskPreflight::Prepared(PreparedParallelTask {
                definition,
                prompt,
                task,
                task_id,
            })
        })
        .collect()
}

pub(crate) fn with_ownership_boundary(prompt: &str, ownership: Option<&str>) -> String {
    match ownership.map(str::trim).filter(|s| !s.is_empty()) {
        Some(boundary) => format!(
            "[Ownership Boundary]\n{boundary}\n\n[Task]\n{prompt}\n\nDo not work outside the ownership boundary unless the parent explicitly asks you to."
        ),
        None => prompt.to_string(),
    }
}

pub(crate) struct SpawnParallelWorker {
    pub(crate) definition: AgentDefinition,
    pub(crate) prompt: String,
    pub(crate) task: ParallelAgentTask,
    pub(crate) task_id: String,
    pub(crate) worktree_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParallelAgentResult {
    pub(crate) task_id: String,
    pub(crate) agent_id: String,
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

pub(crate) async fn stage_spawn_parallel_workers(
    parent_session: &str,
    progress_sink: Option<&Sender<AgentProgress>>,
    tasks: Vec<ParallelAgentTask>,
    registry: &AgentDefinitionRegistry,
    parent: &ParentExecutionContext,
    action_root: Option<&Path>,
) -> (Vec<SpawnParallelWorker>, Vec<ParallelAgentResult>) {
    let mut immediate_results = Vec::new();
    let mut prepared = Vec::new();

    for preflight in prepare_spawn_parallel_tasks(tasks, registry, parent) {
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
        project_spawn_parallel_spawned(
            parent_session,
            progress_sink,
            &definition,
            &task_id,
            prompt.chars().count(),
            task.ownership
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some(),
        )
        .await;
        let worktree_path = match create_spawn_parallel_worktree(
            parent_session,
            action_root,
            &task_id,
            &definition.id,
            &task,
        ) {
            Ok(path) => path,
            Err(result) => {
                immediate_results.push(result);
                continue;
            }
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
    (prepared, immediate_results)
}

pub(crate) async fn run_spawn_parallel_graph(
    parent_session: &str,
    progress_sink: Option<&Sender<AgentProgress>>,
    tasks: Vec<ParallelAgentTask>,
    registry: &AgentDefinitionRegistry,
    parent: &ParentExecutionContext,
    action_root: Option<PathBuf>,
) -> Result<SpawnParallelCollected, String> {
    let (prepared, immediate_results) = stage_spawn_parallel_workers(
        parent_session,
        progress_sink,
        tasks,
        registry,
        parent,
        action_root.as_deref(),
    )
    .await;

    // Fan the prepared workers out through the spawn graph module. Results
    // come back in prepared order, then we append them after immediate
    // pre-flight failures — the existing compatibility ordering.
    let fanned = run_spawn_parallel_workers(prepared, action_root).await?;

    let mut results = immediate_results;
    for result in fanned {
        project_spawn_parallel_result(parent_session, progress_sink, &result).await;
        results.push(result);
    }

    let collected = collect_spawn_parallel_results(parent_session, results);
    tracing::debug!(
        parent_session = %parent_session,
        total = collected.total(),
        succeeded = collected.succeeded(),
        failed = collected.failures,
        overlaps = collected.overlap_warnings.len(),
        "[spawn_parallel_agents] execute exit"
    );
    Ok(collected)
}

pub(crate) struct SpawnParallelCollected {
    pub(crate) results: Vec<ParallelAgentResult>,
    pub(crate) failures: usize,
    pub(crate) overlap_warnings: Vec<serde_json::Value>,
}

impl SpawnParallelCollected {
    pub(crate) fn total(&self) -> usize {
        self.results.len()
    }

    pub(crate) fn succeeded(&self) -> usize {
        self.results.len().saturating_sub(self.failures)
    }
}

pub(crate) fn collect_spawn_parallel_results(
    parent_session: &str,
    mut results: Vec<ParallelAgentResult>,
) -> SpawnParallelCollected {
    annotate_stale_parent_reads(&mut results);
    let overlap_warnings = overlap_warnings_for_results(parent_session, &results);
    let failures = results.iter().filter(|r| !r.success).count();
    SpawnParallelCollected {
        results,
        failures,
        overlap_warnings,
    }
}

pub(crate) fn format_spawn_parallel_success(collected: &SpawnParallelCollected) -> String {
    serde_json::to_string_pretty(&json!({
        "parallel_agents": {
            "total": collected.total(),
            "succeeded": collected.succeeded(),
            "failed": collected.failures,
            "results": collected.results,
            "overlap_warnings": collected.overlap_warnings,
        }
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

pub(crate) async fn project_spawn_parallel_spawned(
    parent_session: &str,
    progress_sink: Option<&Sender<AgentProgress>>,
    definition: &AgentDefinition,
    task_id: &str,
    prompt_chars: usize,
    has_ownership: bool,
) {
    tracing::debug!(
        parent_session = %parent_session,
        task_id = %task_id,
        agent_id = %definition.id,
        prompt_chars,
        has_ownership,
        "[spawn_parallel_agents] publishing_subagent_spawned"
    );
    publish_global(DomainEvent::SubagentSpawned {
        parent_session: parent_session.to_string(),
        agent_id: definition.id.clone(),
        mode: "typed".to_string(),
        task_id: task_id.to_string(),
        prompt_chars,
    });
    if let Some(tx) = progress_sink {
        if let Err(err) = tx
            .send(AgentProgress::SubagentSpawned {
                agent_id: definition.id.clone(),
                task_id: task_id.to_string(),
                mode: "typed".to_string(),
                dedicated_thread: false,
                prompt_chars,
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

pub(crate) async fn project_spawn_parallel_result(
    parent_session: &str,
    progress_sink: Option<&Sender<AgentProgress>>,
    result: &ParallelAgentResult,
) {
    match result {
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
                parent_session: parent_session.to_string(),
                task_id: task_id.clone(),
                agent_id: agent_id.clone(),
                elapsed_ms: *elapsed_ms,
                output_chars: output.as_ref().map(|s| s.chars().count()).unwrap_or(0),
                iterations: *iterations as usize,
            });
            if let Some(tx) = progress_sink {
                if let Err(err) = tx
                    .send(AgentProgress::SubagentCompleted {
                        agent_id: agent_id.clone(),
                        task_id: task_id.clone(),
                        elapsed_ms: *elapsed_ms,
                        iterations: *iterations,
                        output_chars: output.as_ref().map(|s| s.chars().count()).unwrap_or(0),
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
                parent_session: parent_session.to_string(),
                task_id: task_id.clone(),
                agent_id: agent_id.clone(),
                error: message.clone(),
            });
            if let Some(tx) = progress_sink {
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
}

fn annotate_stale_parent_reads(results: &mut [ParallelAgentResult]) {
    if let Some(parent_agent_id) = file_state::current_file_state_agent_id() {
        let child_ids: Vec<String> = results.iter().map(|r| r.task_id.clone()).collect();
        let stale = file_state::parent_stale_files(&parent_agent_id, &child_ids);
        if !stale.is_empty() {
            let stale_strings: Vec<String> =
                stale.iter().map(|p| p.display().to_string()).collect();
            tracing::debug!(
                parent = %parent_agent_id,
                stale_count = stale.len(),
                "[file_state] parent reads stale after child writes"
            );
            for result in results {
                result.stale_parent_reads = stale_strings.clone();
            }
        }
    }
}

fn overlap_warnings_for_results(
    parent_session: &str,
    results: &[ParallelAgentResult],
) -> Vec<serde_json::Value> {
    let per_worker: Vec<(String, Vec<PathBuf>)> = results
        .iter()
        .filter(|r| !r.changed_files.is_empty())
        .map(|r| {
            (
                r.task_id.clone(),
                r.changed_files.iter().map(PathBuf::from).collect(),
            )
        })
        .collect();
    let overlaps = crate::openhuman::agent_orchestration::worktree::detect_overlaps(&per_worker);
    let overlap_warnings: Vec<serde_json::Value> = overlaps
        .iter()
        .map(|(file, workers)| {
            json!({
                "file": file.to_string_lossy(),
                "workers": workers,
            })
        })
        .collect();
    if !overlap_warnings.is_empty() {
        tracing::warn!(
            parent_session = %parent_session,
            overlap_count = overlap_warnings.len(),
            "[spawn_parallel_agents] detected overlapping changed files across workers"
        );
    }
    overlap_warnings
}

pub(crate) async fn run_spawn_parallel_workers(
    prepared: Vec<SpawnParallelWorker>,
    action_root: Option<PathBuf>,
) -> Result<Vec<ParallelAgentResult>, String> {
    let max_concurrency = prepared.len().max(1);
    let action_root_for_workers = action_root.clone();
    crate::openhuman::tinyagents::orchestration::run_parallel_fanout(
        "spawn_parallel_agents",
        prepared,
        max_concurrency,
        move |_i, worker| {
            let repo_root = action_root_for_workers.clone();
            async move { run_one_parallel_task(worker, repo_root).await }
        },
    )
    .await
}

async fn run_one_parallel_task(
    worker: SpawnParallelWorker,
    repo_root: Option<PathBuf>,
) -> ParallelAgentResult {
    let SpawnParallelWorker {
        definition,
        prompt,
        task,
        task_id,
        worktree_path,
    } = worker;
    let started = std::time::Instant::now();
    tracing::debug!(
        task_id = %task_id,
        agent_id = %definition.id,
        toolkit = task.toolkit.as_deref().unwrap_or(""),
        context_chars = task.context.as_ref().map(|s| s.chars().count()).unwrap_or(0),
        prompt_chars = prompt.chars().count(),
        isolated = worktree_path.is_some(),
        "[spawn_parallel_agents] task_start"
    );
    let options = SubagentRunOptions {
        skill_filter_override: None,
        toolkit_override: task.toolkit.clone(),
        context: task.context.clone(),
        model_override: None,
        task_id: Some(task_id.clone()),
        worker_thread_id: None,
        initial_history: None,
        checkpoint_dir: None,
        worktree_action_dir: worktree_path.clone(),
        run_queue: None,
    };
    let run_result = run_subagent(&definition, &prompt, options).await;

    // After the worker finishes, snapshot the worktree's changed files +
    // dirty status so the parent can detect cross-worker overlaps and the UI
    // can surface diff/cleanup actions. Best-effort: a status error degrades
    // to "no changes recorded" rather than failing the task.
    let worktree_str = worktree_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let (changed_files, dirty_status) = match (&worktree_path, &repo_root) {
        (Some(wt), Some(root)) => {
            use crate::openhuman::agent_orchestration::worktree;
            match worktree::status(root, wt) {
                Ok(st) => {
                    tracing::debug!(
                        task_id = %task_id,
                        worktree = %wt.display(),
                        is_dirty = st.is_dirty,
                        changed = st.changed_files.len(),
                        "[spawn_parallel_agents] worktree_post_run_status"
                    );
                    let files = st
                        .changed_files
                        .iter()
                        .map(|p| p.to_string_lossy().to_string())
                        .collect();
                    (files, Some(st.is_dirty))
                }
                Err(err) => {
                    tracing::warn!(
                        task_id = %task_id,
                        worktree = %wt.display(),
                        error = %err,
                        "[spawn_parallel_agents] worktree_status_failed"
                    );
                    (Vec::new(), None)
                }
            }
        }
        _ => (Vec::new(), None),
    };

    match run_result {
        Ok(outcome) => {
            tracing::debug!(
                task_id = %outcome.task_id,
                agent_id = %outcome.agent_id,
                elapsed_ms = outcome.elapsed.as_millis() as u64,
                iterations = outcome.iterations,
                output_chars = outcome.output.chars().count(),
                "[spawn_parallel_agents] task_success"
            );
            ParallelAgentResult {
                task_id: outcome.task_id,
                agent_id: outcome.agent_id,
                success: true,
                output: Some(outcome.output),
                error: None,
                ownership: task.ownership,
                elapsed_ms: outcome.elapsed.as_millis() as u64,
                iterations: outcome.iterations as u32,
                stale_parent_reads: Vec::new(),
                worktree_path: worktree_str,
                changed_files,
                dirty_status,
            }
        }
        Err(err) => {
            tracing::debug!(
                task_id = %task_id,
                agent_id = %definition.id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %err,
                "[spawn_parallel_agents] task_error"
            );
            ParallelAgentResult {
                task_id,
                agent_id: definition.id,
                success: false,
                output: None,
                error: Some(err.to_string()),
                ownership: task.ownership,
                elapsed_ms: started.elapsed().as_millis() as u64,
                iterations: 0,
                stale_parent_reads: Vec::new(),
                worktree_path: worktree_str,
                changed_files,
                dirty_status,
            }
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct SpawnParallelState;

pub(crate) enum SpawnParallelUpdate {
    Noop,
}

fn noop_node(
    _state: SpawnParallelState,
    _ctx: NodeContext,
) -> impl std::future::Future<Output = tinyagents::Result<NodeResult<SpawnParallelUpdate>>> + Send {
    async move { Ok(NodeResult::Update(SpawnParallelUpdate::Noop)) }
}

/// Build the fixed `spawn_parallel_agents` graph scaffold.
///
/// The node order is intentionally static for topology export:
///
/// `validate -> dispatch -> worker -> collect -> finalize`
pub(crate) fn build_spawn_parallel_graph()
-> Result<CompiledGraph<SpawnParallelState, SpawnParallelUpdate>, String> {
    GraphBuilder::<SpawnParallelState, SpawnParallelUpdate>::new()
        .set_reducer(ClosureStateReducer::new(
            |state: SpawnParallelState, _update: SpawnParallelUpdate| Ok(state),
        ))
        .add_node("validate", noop_node)
        .add_node("dispatch", noop_node)
        .add_node("worker", noop_node)
        .add_node("collect", noop_node)
        .add_node("finalize", noop_node)
        .add_edge("validate", "dispatch")
        .add_edge("dispatch", "worker")
        .add_edge("worker", "collect")
        .add_edge("collect", "finalize")
        .set_entry("validate")
        .set_finish("finalize")
        .compile()
        .map_err(|e| format!("spawn_parallel_agents graph compile failed: {e}"))
}

/// Structure-only topology of the `spawn_parallel_agents` graph.
pub(crate) fn spawn_parallel_graph_topology() -> Result<GraphTopology, String> {
    Ok(build_spawn_parallel_graph()?.topology())
}
