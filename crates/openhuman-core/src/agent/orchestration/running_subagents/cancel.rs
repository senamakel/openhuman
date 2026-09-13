//! Aborting sub-agents: by task id (the user-facing "Cancel" affordance), by
//! durable session id, by owning parent thread (thread deletion), or all at
//! once (a full thread purge).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tinyagents_harness::ids::TaskId;

use super::registry::registry;
use super::resolve::{task_id_for_session, task_id_for_session_in_workspace};
use super::task_ledger::record_cancelled;

/// Metadata captured when a sub-agent is cancelled, so the caller can surface
/// the cancellation back in the parent chat (record a "cancelled" completion
/// for idle-gated delivery).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CancelledSubagent {
    pub(crate) agent_id: String,
    pub(crate) parent_session: String,
    pub(crate) subagent_session_id: Option<String>,
    pub(crate) workspace_dir: PathBuf,
    pub(crate) parent_thread_id: Option<String>,
}

/// Abort and drop the sub-agent with `task_id`, returning its metadata so the
/// caller can deliver a "cancelled" notice into the parent chat. Returns `None`
/// if no such sub-agent is registered (already finished, or unknown id).
///
/// Unlike the parent-session-owned steering and close paths, this is keyed by
/// `task_id` alone with no ownership check — it backs the user-facing "Cancel"
/// affordance, and the desktop user owns every sub-agent in their own core.
pub(crate) fn cancel_by_task(task_id: &str) -> Option<CancelledSubagent> {
    let cancelled = registry().cancel_trusted(&TaskId::new(task_id)).ok()?;
    let metadata = cancelled.metadata;
    record_cancelled(&metadata.workspace_dir, task_id);
    log::debug!(
        "[running_subagents] cancel_by_task task_id={} agent_id={} parent_thread_id={:?} live_entries={}",
        task_id,
        metadata.agent_id,
        metadata.parent_thread_id,
        registry()
            .len()
            .expect("detached task registry lock poisoned")
    );
    Some(CancelledSubagent {
        agent_id: metadata.agent_id,
        parent_session: cancelled.owner_id,
        subagent_session_id: metadata.subagent_session_id,
        workspace_dir: metadata.workspace_dir,
        parent_thread_id: metadata.parent_thread_id,
    })
}

pub(crate) fn cancel_by_session(
    subagent_session_id: &str,
    parent_session: &str,
) -> Option<CancelledSubagent> {
    let task_id = task_id_for_session(subagent_session_id, parent_session).ok()?;
    cancel_by_task(&task_id)
}

pub(crate) fn cancel_by_session_in_workspace(
    subagent_session_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
) -> Option<CancelledSubagent> {
    let task_id =
        task_id_for_session_in_workspace(subagent_session_id, parent_session, workspace_dir)
            .ok()?;
    cancel_by_task(&task_id)
}

/// Abort and drop every running sub-agent whose parent chat thread is
/// `thread_id`. Called when that thread is deleted so detached children don't
/// keep running (and later try to deliver) against a thread that no longer
/// exists. Returns the number of sub-agents cancelled.
pub(crate) fn cancel_for_thread(thread_id: &str) -> usize {
    let cancelled = registry()
        .cancel_where(|metadata| metadata.parent_thread_id.as_deref() == Some(thread_id))
        .expect("detached task registry lock poisoned");
    for entry in &cancelled {
        record_cancelled(&entry.metadata.workspace_dir, entry.task_id.as_str());
    }
    let count = cancelled.len();
    log::debug!(
        "[running_subagents] cancel_for_thread thread_id={} cancelled={} live_entries={}",
        thread_id,
        count,
        registry()
            .len()
            .expect("detached task registry lock poisoned")
    );
    count
}

/// Abort and drop **every** registered sub-agent. Called on a full thread purge
/// where no parent thread survives. Returns the **distinct parent thread ids**
/// that had sub-agents, so the purge path can tombstone them in
/// [`super::super::background_completions`] and drop any straggler completion
/// that wins the cooperative-abort race. Headless sub-agents (no parent thread)
/// are still aborted but contribute no id.
pub(crate) fn cancel_all() -> Vec<String> {
    let cancelled = registry()
        .cancel_all()
        .expect("detached task registry lock poisoned");
    let count = cancelled.len();
    let mut thread_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for entry in cancelled {
        record_cancelled(&entry.metadata.workspace_dir, entry.task_id.as_str());
        if let Some(thread_id) = entry.metadata.parent_thread_id {
            if seen.insert(thread_id.clone()) {
                thread_ids.push(thread_id);
            }
        }
    }
    log::debug!(
        "[running_subagents] cancel_all cancelled={} distinct_threads={}",
        count,
        thread_ids.len()
    );
    thread_ids
}

#[allow(dead_code)]
pub(crate) fn prune(task_id: &str) {
    let _ = registry().cancel_trusted(&TaskId::new(task_id));
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
