//! Blocking-with-timeout collection of a sub-agent's terminal result, with a
//! durable task-store fallback for a sub-agent this process never registered
//! in-memory (e.g. after a core restart).

use std::path::Path;
use std::time::Duration;

use crate::agent::tinyagents::orchestration::DetachedTaskRegistryError;
use tinyagents_harness::ids::TaskId;

use super::registry::{registry, SubagentStatus};
use super::task_ledger::{record_to_status, task_record_for_task_in_workspace, task_status_label};
use crate::agent::tinyagents::orchestration::DetachedTaskWaitOutcome;

/// Why a wait could not be set up.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WaitError {
    Unknown,
    NotOwned,
}

pub(crate) fn wait_error_from_registry(error: DetachedTaskRegistryError) -> WaitError {
    match error {
        DetachedTaskRegistryError::NotOwned => WaitError::NotOwned,
        _ => WaitError::Unknown,
    }
}

/// Result of waiting on a sub-agent.
#[derive(Debug)]
pub(crate) enum WaitOutcome {
    /// The sub-agent reached a terminal status (entry pruned).
    Terminal(SubagentStatus),
    /// The timeout elapsed first; the entry is left intact so the parent can
    /// wait again. Carries the latest (non-terminal) status snapshot.
    TimedOut(SubagentStatus),
}

/// Block until `task_id` reaches a terminal status or `timeout` elapses.
pub(crate) async fn wait(
    task_id: &str,
    parent_session: &str,
    timeout: Duration,
) -> Result<WaitOutcome, WaitError> {
    match registry()
        .wait(&TaskId::new(task_id), parent_session, timeout)
        .await
    {
        Ok(DetachedTaskWaitOutcome::Terminal(status)) => Ok(WaitOutcome::Terminal(status)),
        Ok(DetachedTaskWaitOutcome::TimedOut(status)) => Ok(WaitOutcome::TimedOut(status)),
        Err(DetachedTaskRegistryError::StatusChannelClosed) => {
            Ok(WaitOutcome::Terminal(SubagentStatus::Failed {
                error: "sub-agent task ended without reporting a result".to_string(),
            }))
        }
        Err(error) => Err(wait_error_from_registry(error)),
    }
}

pub(crate) async fn wait_in_workspace(
    task_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
    timeout: Duration,
) -> Result<WaitOutcome, WaitError> {
    match wait(task_id, parent_session, timeout).await {
        Ok(outcome) => return Ok(outcome),
        Err(WaitError::NotOwned) => return Err(WaitError::NotOwned),
        Err(WaitError::Unknown) => {}
    }

    let record = task_record_for_task_in_workspace(workspace_dir, task_id, parent_session)?;
    log::debug!(
        "[running_subagents] resolved wait from task store task_id={} status={} workspace_dir={}",
        task_id,
        task_status_label(record.status),
        workspace_dir.display()
    );
    Ok(record_to_status(record))
}
