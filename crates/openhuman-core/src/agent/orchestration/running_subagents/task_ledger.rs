//! Durable per-workspace task store plumbing and the typed lifecycle ledger
//! that mirrors each detached sub-agent's status into it (issue #4249).
//!
//! The caching and the durable→memory fallback are TinyAgents'
//! (`TaskStoreRegistry` / `open_jsonl_task_store_or_memory`): opening a second
//! store over the same append log would give two writers with independently
//! replayed state, and a workspace that cannot be written should degrade to an
//! in-memory ledger rather than take orchestration down. What stays here is the
//! path layout, the record shape, and the reconciliation of orphaned tasks on
//! boot.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::agent::tinyagents::orchestration::{
    open_jsonl_task_store_or_memory, reconcile_orphaned_tasks, InMemoryTaskStore,
    OrchestrationTaskFilter, OrchestrationTaskKind, OrchestrationTaskRecord,
    OrchestrationTaskResult, OrchestrationTaskSpec, OrchestrationTaskStatus, TaskStore,
    TaskStoreRegistry,
};
use tinyagents_harness::ids::TaskId;

use super::registry::DETACHED_LEDGER_TIMEOUT_MS;
use super::wait::{WaitError, WaitOutcome};
use super::SubagentStatus;

/// Where a workspace's detached-task ledger lives.
///
/// A product path, not a generic one: TinyAgents opens whatever file it is
/// given, and this is where OpenHuman keeps it.
fn task_store_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir
        .join(".openhuman")
        .join("orchestration_tasks.jsonl")
}

#[cfg(test)]
fn default_task_store_workspace() -> PathBuf {
    crate::config::default_root_openhuman_dir()
        .map(|root| root.join("workspace"))
        .unwrap_or_else(|_| PathBuf::from(".openhuman").join("workspace"))
}

/// Process-wide typed lifecycle ledger for detached sub-agents (issue #4249),
/// one durable store per workspace.
static TASK_STORES: OnceLock<TaskStoreRegistry<PathBuf>> = OnceLock::new();

fn task_stores() -> &'static TaskStoreRegistry<PathBuf> {
    TASK_STORES.get_or_init(|| {
        TaskStoreRegistry::new(|workspace_dir: &PathBuf| {
            open_jsonl_task_store_or_memory(&task_store_path(workspace_dir))
        })
    })
}

/// The ledger for `workspace_dir`, opening it on first use.
///
/// A poisoned registry lock is degraded to a throwaway in-memory store rather
/// than propagated: every caller here is a best-effort bookkeeping path, and a
/// panic in an unrelated task must not turn sub-agent spawning into a second
/// panic.
pub(crate) fn task_store_for_workspace(workspace_dir: &Path) -> Arc<dyn TaskStore> {
    let key = workspace_dir.to_path_buf();
    match task_stores().get_or_open(&key) {
        Ok(store) => store,
        Err(err) => {
            log::warn!(
                "[running_subagents] task store registry unavailable for {}; using a detached in-memory ledger: {}",
                workspace_dir.display(),
                err
            );
            Arc::new(InMemoryTaskStore::new())
        }
    }
}

#[cfg(test)]
fn task_store() -> Arc<dyn TaskStore> {
    let workspace = default_task_store_workspace();
    task_store_for_workspace(&workspace)
}

/// Record a freshly-spawned sub-agent in the store (`Pending` → `Running`).
/// Insert errors (e.g. a re-used task id across tests) are intentionally ignored.
pub(crate) fn record_spawned(
    task_id: &str,
    agent_id: &str,
    parent_session: &str,
    session_parent_prefix: Option<&str>,
    subagent_session_id: Option<&str>,
    workspace_dir: &Path,
    parent_thread_id: Option<&str>,
) {
    let store = task_store_for_workspace(workspace_dir);
    let root_run_id = session_parent_prefix
        .and_then(|prefix| prefix.split("__").next())
        .filter(|root| !root.is_empty())
        .unwrap_or(parent_session);
    let mut spec = OrchestrationTaskSpec::new(
        task_id.to_string(),
        OrchestrationTaskKind::SubAgent {
            agent: agent_id.to_string(),
        },
    )
    .with_lineage(parent_session.to_string(), root_run_id.to_string())
    .with_timeout_ms(DETACHED_LEDGER_TIMEOUT_MS)
    .with_metadata("parentSession", parent_session.to_string())
    .with_metadata("rootSession", root_run_id.to_string())
    .with_metadata(
        "defaultWaitTimeoutMs",
        DETACHED_LEDGER_TIMEOUT_MS.to_string(),
    )
    .with_metadata("workspaceDir", workspace_dir.display().to_string());
    if let Some(session_parent_prefix) = session_parent_prefix {
        spec = spec.with_metadata("sessionParentPrefix", session_parent_prefix.to_string());
    }
    if let Some(parent_thread_id) = parent_thread_id {
        spec = spec
            .with_thread(parent_thread_id.to_string())
            .with_metadata("parentThreadId", parent_thread_id.to_string());
    }
    if let Some(subagent_session_id) = subagent_session_id {
        spec = spec.with_metadata("subagentSessionId", subagent_session_id.to_string());
    }
    let _ = store.insert(spec);
    let _ = store.mark_running(&TaskId::new(task_id));
}

/// Mirror a child's published [`SubagentStatus`] into the typed store. Transition
/// errors (already terminal / cancelled) are ignored — first writer wins.
pub(crate) fn record_status(workspace_dir: &Path, task_id: &str, status: &SubagentStatus) {
    let store = task_store_for_workspace(workspace_dir);
    let id = TaskId::new(task_id);
    log::debug!(
        "[running_subagents] recording task status task_id={} workspace_dir={} terminal={}",
        task_id,
        workspace_dir.display(),
        status.is_terminal()
    );
    match status {
        SubagentStatus::Completed { output, .. } => {
            let _ = store.complete(&id, OrchestrationTaskResult::text(output.clone()));
        }
        SubagentStatus::Failed { error } => {
            let _ = store.fail(&id, error.clone());
        }
        SubagentStatus::AwaitingUser { .. } => {
            let _ = store.mark_awaiting(&id);
        }
        SubagentStatus::Running => {}
    }
}

/// Record a cancellation (`CancelRequested` → `Cancelled`) for `task_id`.
pub(crate) fn record_cancelled(workspace_dir: &Path, task_id: &str) {
    let store = task_store_for_workspace(workspace_dir);
    let id = TaskId::new(task_id);
    log::debug!(
        "[running_subagents] recording task cancellation task_id={} workspace_dir={}",
        task_id,
        workspace_dir.display()
    );
    let _ = store.request_cancel(&id);
    let _ = store.mark_cancelled(&id);
}

pub(crate) fn list_task_records(workspace_dir: &Path) -> Vec<OrchestrationTaskRecord> {
    let store = task_store_for_workspace(workspace_dir);
    store.list(OrchestrationTaskFilter::default().with_kind("sub_agent"))
}

/// Restart/resume reconciliation for detached sub-agents (issue #4249 / 07.2
/// steps 2 & 4).
///
/// A detached sub-agent runs as a `tokio` task owned by the process that spawned
/// it. When the core restarts, that task — and its live [`tokio::task::AbortHandle`] /
/// [`tinyagents_harness::CancellationToken`] — is gone, but the durable
/// `JsonlTaskStore` still holds a non-terminal (`Pending`/`Running`/`Awaiting`/
/// `CancelRequested`) record for it. Such a record is **orphaned**: there is no
/// live executor to re-attach to (OpenHuman spawns child processes, so an
/// in-flight run from a dead parent cannot be resumed), and the run-ledger
/// finalizer never observed a terminal event, so it would otherwise render as a
/// perpetual "running" entry.
///
/// This scans the workspace-scoped store for those orphans and reconciles each
/// to a terminal state — `Cancelled` if a cancel had been requested, otherwise
/// `Failed` with an "orphaned by restart" reason — then emits the existing typed
/// terminal lifecycle event ([`crate::agent::orchestration::subagent_events::publish_subagent_failed`])
/// so the run ledger finalizes. Best-effort and non-fatal: per-task transition
/// errors (e.g. a record that raced to terminal) are logged and skipped, and a
/// store-open failure simply reconciles nothing. Returns the count reconciled.
pub(crate) fn reconcile_orphaned_tasks_on_boot(workspace_dir: &Path) -> usize {
    let store = task_store_for_workspace(workspace_dir);

    // The sweep itself — which statuses are live, and which terminal state each
    // becomes — is TinyAgents'. What stays here is the reason a *sub-agent*
    // orphan carries, and the lifecycle event that finalizes OpenHuman's run
    // ledger afterwards.
    let report = reconcile_orphaned_tasks(
        store.as_ref(),
        OrchestrationTaskFilter::default().with_kind("sub_agent"),
        &|record| orphaned_subagent_reason(record.status),
    );

    if report.is_empty() {
        log::debug!(
            "[running_subagents] reconcile found no orphaned sub-agent tasks workspace_dir={}",
            workspace_dir.display()
        );
        return 0;
    }

    for task in report.settled() {
        let task_id = task.task_id.as_str().to_string();
        let prior = task_status_label(task.prior_status);
        let reason = orphaned_subagent_reason(task.prior_status);
        let parent_session = record_parent_session(&task.record)
            .unwrap_or_default()
            .to_string();
        let agent_id = record_agent_id(&task.record);
        // Reuse the 05.2 typed terminal lifecycle helper so the run ledger
        // finalizes exactly as it would for a live failure.
        crate::agent::orchestration::subagent_events::publish_subagent_failed(
            parent_session,
            task_id.clone(),
            agent_id,
            reason,
        );
        log::info!(
            "[running_subagents] reconciled orphaned sub-agent task_id={} prior_status={} -> terminal",
            task_id,
            prior
        );
    }

    let reconciled = report.reconciled_count();
    log::info!(
        "[running_subagents] reconciled {reconciled} orphaned sub-agent task(s) on boot workspace_dir={} errors={}",
        workspace_dir.display(),
        report.error_count()
    );
    reconciled
}

/// The reason an orphaned sub-agent record is settled with.
///
/// Built in one place because it is written twice — into the store by the
/// reconciler, and into the lifecycle event the run ledger reads. If those two
/// ever disagreed, the ledger would explain a failure differently from the
/// record behind it.
fn orphaned_subagent_reason(prior_status: OrchestrationTaskStatus) -> String {
    format!(
        "sub-agent orphaned by core restart (was `{}`)",
        task_status_label(prior_status)
    )
}

pub(crate) fn record_parent_session(record: &OrchestrationTaskRecord) -> Option<&str> {
    record
        .spec
        .metadata
        .get("parentSession")
        .map(String::as_str)
}

pub(crate) fn record_subagent_session_id(record: &OrchestrationTaskRecord) -> Option<&str> {
    record
        .spec
        .metadata
        .get("subagentSessionId")
        .map(String::as_str)
}

pub(crate) fn record_agent_id(record: &OrchestrationTaskRecord) -> String {
    match &record.spec.kind {
        OrchestrationTaskKind::SubAgent { agent } => agent.clone(),
        _ => "subagent".to_string(),
    }
}

pub(crate) fn task_record_for_task_in_workspace(
    workspace_dir: &Path,
    task_id: &str,
    parent_session: &str,
) -> Result<OrchestrationTaskRecord, WaitError> {
    let id = TaskId::new(task_id);
    let Some(record) = task_store_for_workspace(workspace_dir).get(&id) else {
        return Err(WaitError::Unknown);
    };
    if !matches!(record.spec.kind, OrchestrationTaskKind::SubAgent { .. }) {
        return Err(WaitError::Unknown);
    }
    if record_parent_session(&record) != Some(parent_session) {
        return Err(WaitError::NotOwned);
    }
    Ok(record)
}

pub(crate) fn record_to_status(record: OrchestrationTaskRecord) -> WaitOutcome {
    match record.status {
        OrchestrationTaskStatus::Completed => {
            let output = record
                .result
                .and_then(|result| {
                    result
                        .text
                        .or_else(|| result.output.map(|output| output.to_string()))
                })
                .unwrap_or_default();
            WaitOutcome::Terminal(SubagentStatus::Completed {
                output,
                iterations: 0,
            })
        }
        OrchestrationTaskStatus::Awaiting => WaitOutcome::Terminal(SubagentStatus::AwaitingUser {
            question: record.error.unwrap_or_else(|| {
                "sub-agent is awaiting user input; no clarification text was available from the durable task store".to_string()
            }),
        }),
        OrchestrationTaskStatus::Failed
        | OrchestrationTaskStatus::TimedOut
        | OrchestrationTaskStatus::Abandoned => WaitOutcome::Terminal(SubagentStatus::Failed {
            error: record.error.unwrap_or_else(|| {
                format!(
                    "sub-agent reached durable task status `{}`",
                    task_status_label(record.status)
                )
            }),
        }),
        OrchestrationTaskStatus::Cancelled => WaitOutcome::Terminal(SubagentStatus::Failed {
            error: "sub-agent was cancelled".to_string(),
        }),
        OrchestrationTaskStatus::Pending
        | OrchestrationTaskStatus::Running
        | OrchestrationTaskStatus::CancelRequested => WaitOutcome::TimedOut(SubagentStatus::Running),
    }
}

pub(crate) fn task_status_label(status: OrchestrationTaskStatus) -> &'static str {
    match status {
        OrchestrationTaskStatus::Pending => "pending",
        OrchestrationTaskStatus::Running => "running",
        OrchestrationTaskStatus::Awaiting => "awaiting",
        OrchestrationTaskStatus::Completed => "completed",
        OrchestrationTaskStatus::Failed => "failed",
        OrchestrationTaskStatus::CancelRequested => "cancel_requested",
        OrchestrationTaskStatus::Cancelled => "cancelled",
        OrchestrationTaskStatus::TimedOut => "timed_out",
        OrchestrationTaskStatus::Abandoned => "abandoned",
    }
}

/// Snapshot the typed lifecycle records, optionally scoped to a `parent_session`.
#[cfg(test)]
pub(crate) fn task_records(parent_session: Option<&str>) -> Vec<OrchestrationTaskRecord> {
    let _ = task_store();
    let stores: Vec<Arc<dyn TaskStore>> = task_stores().values().unwrap_or_default();
    let all: Vec<OrchestrationTaskRecord> = stores
        .into_iter()
        .flat_map(|store| store.list(OrchestrationTaskFilter::default()))
        .collect();
    log::trace!(
        "[running_subagents] task_records loaded records={} parent_session_filter={:?}",
        all.len(),
        parent_session
    );
    match parent_session {
        Some(ps) => all
            .into_iter()
            .filter(|r| r.spec.metadata.get("parentSession").map(String::as_str) == Some(ps))
            .collect(),
        None => all,
    }
}
