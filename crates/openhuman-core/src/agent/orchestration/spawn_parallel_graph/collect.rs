//! Collecting fanned-out worker results into the tool's final shape: overlap
//! detection, stale-parent-read annotation, progress projection for
//! completed/failed workers, and the outcome enum the graph run resolves to.

use std::path::PathBuf;

use serde_json::json;
use tokio::sync::mpsc::Sender;

use crate::agent::file_state;
use crate::agent::progress::AgentProgress;

use super::request::SpawnParallelTaskValidationError;
use super::types::ParallelAgentResult;

#[derive(Clone)]
pub(crate) struct SpawnParallelCollected {
    pub(crate) results: Vec<ParallelAgentResult>,
    pub(crate) failures: usize,
    pub(crate) overlap_warnings: Vec<serde_json::Value>,
}

pub(crate) enum SpawnParallelGraphOutcome {
    Collected(SpawnParallelCollected),
    InvalidRequest(SpawnParallelTaskValidationError),
    Rejected(String),
    Cancelled(String),
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
            crate::agent::orchestration::subagent_events::publish_subagent_completed(
                parent_session.to_string(),
                task_id.clone(),
                agent_id.clone(),
                *elapsed_ms,
                output.as_ref().map(|s| s.chars().count()).unwrap_or(0),
                *iterations as usize,
            );
            if let Some(tx) = progress_sink {
                if let Err(err) = tx
                    .send(AgentProgress::SubagentCompleted {
                        agent_id: agent_id.clone(),
                        task_id: task_id.clone(),
                        elapsed_ms: *elapsed_ms,
                        iterations: *iterations,
                        output_chars: output.as_ref().map(|s| s.chars().count()).unwrap_or(0),
                        output: output.clone().unwrap_or_default(),
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
            crate::agent::orchestration::subagent_events::publish_subagent_failed(
                parent_session.to_string(),
                task_id.clone(),
                agent_id.clone(),
                message.clone(),
            );
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
    let overlaps = crate::agent::orchestration::worktree::detect_overlaps(&per_worker);
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
