//! Read-only rosters over the sub-agent registry: a compact snapshot type for
//! ambient prompt injection, and the durable+live merge used to render the
//! `[active_subagents]` context block.

use super::registry::{registry, SubagentStatus};

/// Compact, read-only view of one registered sub-agent, for ambient injection
/// into a parent's turn context (see [`active_subagents_context_block`]).
#[derive(Debug, Clone)]
pub(crate) struct SubagentSnapshot {
    /// Worker *type* (e.g. `researcher`). Not unique — two parallel researchers
    /// share this; disambiguate on `subagent_session_id` / `task_id`.
    pub(crate) agent_id: String,
    /// Durable, stable per-worker reference the prompt steers/waits/closes by.
    pub(crate) subagent_session_id: Option<String>,
    /// Transient registry key.
    pub(crate) task_id: String,
    /// Stable status label: `running` / `awaiting_user` / `completed` / `failed`.
    pub(crate) status: &'static str,
}

/// Snapshot the sub-agents registered under `parent_session`, with each status
/// read live from its watch channel. Read-only: it takes the registry lock only
/// long enough to clone out the small summaries, never blocks on a child, and
/// never mutates the table. Ordered by `agent_id` then `task_id` so the rendered
/// roster is stable across turns (the underlying map is unordered).
pub(crate) fn snapshot_for_parent(parent_session: &str) -> Vec<SubagentSnapshot> {
    let mut out: Vec<SubagentSnapshot> = registry()
        .snapshots(Some(parent_session))
        .expect("detached task registry lock poisoned")
        .into_iter()
        .map(|entry| {
            let status = match &entry.status {
                SubagentStatus::Running => "running",
                SubagentStatus::Completed { .. } => "completed",
                SubagentStatus::AwaitingUser { .. } => "awaiting_user",
                SubagentStatus::Failed { .. } => "failed",
            };
            SubagentSnapshot {
                agent_id: entry.metadata.agent_id,
                subagent_session_id: entry.metadata.subagent_session_id,
                task_id: entry.task_id.as_str().to_string(),
                status,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.agent_id
            .cmp(&b.agent_id)
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
    out
}

/// Most-recent durable sessions surfaced in the roster when they are not in
/// the live registry (cold boot / later turn). Bounds prompt growth on
/// threads with a long delegation history.
const DURABLE_ROSTER_CAP: usize = 12;

/// Build the ambient `[active_subagents]` block prepended to a parent's turn
/// context. Returns `None` when the parent owns no sub-agents at all, so the
/// block only appears when it is actionable — turns for agents that never
/// spawn are untouched. Mirrors the thread-goal `[active_goal]` block: it
/// rides the per-turn context (not the cached system-prompt prefix), so it
/// reflects live status every turn.
///
/// The roster merges two sources:
/// 1. the in-memory registry (live async workers spawned this process), and
/// 2. the durable per-workspace `subagent_sessions` store — workers from
///    EARLIER turns / process lifetimes. Without this second source a
///    cold-booted parent had no idea its previous sub-agents existed and
///    would re-delegate from scratch instead of resuming by
///    `subagent_session_id` (the "fresh context from day 0" bug).
pub(crate) fn active_subagents_context_block(
    parent_session: &str,
    workspace_dir: &std::path::Path,
) -> Option<String> {
    let workers = snapshot_for_parent(parent_session);

    // Durable sessions not already represented by a live registry entry.
    let live_session_ids: std::collections::HashSet<String> = workers
        .iter()
        .filter_map(|w| w.subagent_session_id.clone())
        .collect();
    let store = crate::agent::orchestration::subagent_sessions::SubagentSessionStore {
        workspace_dir: workspace_dir.to_path_buf(),
    };
    let durable: Vec<_> = match crate::agent::orchestration::subagent_sessions::list_for_parent(
        &store,
        parent_session,
        None,
    ) {
        Ok(sessions) => sessions
            .into_iter()
            .filter(|s| {
                use crate::agent::orchestration::subagent_sessions::DurableSubagentStatus;
                s.status != DurableSubagentStatus::Closed
                    && !live_session_ids.contains(&s.subagent_session_id)
            })
            .take(DURABLE_ROSTER_CAP)
            .collect(),
        Err(err) => {
            log::warn!(
                    "[running_subagents] durable roster load failed parent_session={parent_session} error={err}"
                );
            Vec::new()
        }
    };

    if workers.is_empty() && durable.is_empty() {
        return None;
    }
    let mut block = format!(
        "[active_subagents]\n\
         You have {} sub-agent worker(s) for this conversation (live and/or from earlier \
         turns). This is your authoritative roster — trust it over memory. Track each by \
         subagent_session_id; use wait_subagent to collect a `completed` one, steer_subagent \
         to redirect a `running` one, continue_subagent to answer an `awaiting_user` one or \
         to RESUME an `idle` one with a follow-up (it keeps its full prior context — do NOT \
         re-delegate the same task from scratch), close_subagent when done, and \
         list_subagents to re-enumerate. Never fabricate a result for a worker still running \
         or one that has failed.\n",
        workers.len() + durable.len()
    );
    for w in &workers {
        let session = w.subagent_session_id.as_deref().unwrap_or("(none)");
        block.push_str(&format!(
            "- {} · session={} · task={} · status={}\n",
            w.agent_id, session, w.task_id, w.status
        ));
    }
    for s in &durable {
        use crate::agent::orchestration::subagent_sessions::DurableSubagentStatus;
        let status = match s.status {
            DurableSubagentStatus::Running => "running",
            DurableSubagentStatus::Idle => "idle",
            DurableSubagentStatus::AwaitingUser => "awaiting_user",
            DurableSubagentStatus::Failed => "failed",
            DurableSubagentStatus::Closed => "closed",
        };
        let task = s.current_task_id.as_deref().unwrap_or("(none)");
        block.push_str(&format!(
            "- {} · session={} · task={} · status={} · about: {}\n",
            s.agent_id, s.subagent_session_id, task, status, s.task_title
        ));
    }
    block.push_str("[/active_subagents]\n\n");
    Some(block)
}
