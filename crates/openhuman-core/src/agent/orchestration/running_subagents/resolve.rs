//! Resolving a durable `subagent_session_id` (or a transient `task_id`) back
//! to the live registry entry or, failing that, the durable task-store record
//! — used by tool/control paths that only know the stable session id a
//! sub-agent is addressed by.

use std::path::Path;

use tinyagents_harness::ids::TaskId;

use crate::agent::tinyagents::orchestration::OrchestrationTaskRecord;

use super::registry::{registry, SubagentResumeRef};
use super::task_ledger::{
    list_task_records, record_agent_id, record_parent_session, record_subagent_session_id,
    task_record_for_task_in_workspace,
};
use super::wait::WaitError;

/// Resolve a durable `subagent_session_id` to the currently-running transient
/// `task_id`, enforcing parent-session ownership.
pub(crate) fn task_id_for_session(
    subagent_session_id: &str,
    parent_session: &str,
) -> Result<String, WaitError> {
    let mut saw_unowned = false;
    let mut owned_terminal: Option<String> = None;
    for snapshot in registry()
        .snapshots(None)
        .expect("detached task registry lock poisoned")
        .into_iter()
        .filter(|snapshot| {
            snapshot.metadata.subagent_session_id.as_deref() == Some(subagent_session_id)
        })
    {
        if snapshot.owner_id != parent_session {
            saw_unowned = true;
            continue;
        }
        let task_id = snapshot.task_id.as_str().to_string();
        if !snapshot.status.is_terminal() {
            return Ok(task_id);
        }
        owned_terminal.get_or_insert(task_id);
    }
    if let Some(task_id) = owned_terminal {
        return Ok(task_id);
    }
    if saw_unowned {
        return Err(WaitError::NotOwned);
    }
    Err(WaitError::Unknown)
}

pub(crate) fn task_id_for_session_in_workspace(
    subagent_session_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
) -> Result<String, WaitError> {
    match task_id_for_session(subagent_session_id, parent_session) {
        Ok(task_id) => return Ok(task_id),
        Err(WaitError::NotOwned) => return Err(WaitError::NotOwned),
        Err(WaitError::Unknown) => {}
    }

    let mut saw_unowned = false;
    let mut matches: Vec<OrchestrationTaskRecord> = list_task_records(workspace_dir)
        .into_iter()
        .filter(|record| record_subagent_session_id(record) == Some(subagent_session_id))
        .collect();
    matches.sort_by_key(|item| std::cmp::Reverse(item.updated_at));

    for record in matches {
        if record_parent_session(&record) != Some(parent_session) {
            saw_unowned = true;
            continue;
        }
        let task_id = record.spec.task_id.as_str().to_string();
        log::debug!(
            "[running_subagents] resolved session from task store subagent_session_id={} task_id={} workspace_dir={}",
            subagent_session_id,
            task_id,
            workspace_dir.display()
        );
        return Ok(task_id);
    }
    if saw_unowned {
        return Err(WaitError::NotOwned);
    }
    Err(WaitError::Unknown)
}

pub(crate) fn resume_ref_for_task(
    task_id: &str,
    parent_session: &str,
) -> Result<SubagentResumeRef, WaitError> {
    let snapshot = registry()
        .snapshot(&TaskId::new(task_id), parent_session)
        .map_err(super::wait::wait_error_from_registry)?;
    Ok(SubagentResumeRef {
        task_id: task_id.to_string(),
        agent_id: snapshot.metadata.agent_id,
        subagent_session_id: snapshot.metadata.subagent_session_id,
    })
}

pub(crate) fn resume_ref_for_task_in_workspace(
    task_id: &str,
    parent_session: &str,
    workspace_dir: &Path,
) -> Result<SubagentResumeRef, WaitError> {
    match resume_ref_for_task(task_id, parent_session) {
        Ok(reference) => return Ok(reference),
        Err(WaitError::NotOwned) => return Err(WaitError::NotOwned),
        Err(WaitError::Unknown) => {}
    }

    let record = task_record_for_task_in_workspace(workspace_dir, task_id, parent_session)?;
    log::debug!(
        "[running_subagents] resolved resume ref from task store task_id={} workspace_dir={}",
        task_id,
        workspace_dir.display()
    );
    Ok(SubagentResumeRef {
        task_id: task_id.to_string(),
        agent_id: record_agent_id(&record),
        subagent_session_id: record_subagent_session_id(&record).map(ToOwned::to_owned),
    })
}
