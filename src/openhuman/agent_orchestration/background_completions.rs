//! Per-session queue of *finished* detached background sub-agents
//! (`spawn_async_subagent`) awaiting delivery back into the chat.
//!
//! A detached sub-agent runs fire-and-forget; when it finishes, its result is
//! recorded here keyed by `parent_session`. The delivery subsystem
//! ([`super::background_delivery`]) drains the queue **when the session is
//! idle** (never mid-turn) and runs a single *system* turn on the parent chat
//! thread carrying every result ready at that moment — batched, with each one
//! tagged by its sub-agent process id. This module owns only the queue + the
//! notice formatting.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// One finished background sub-agent's deliverable result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedBackgroundAgent {
    /// Spawn process id (`sub-…`) — the tag the agent uses to reference it.
    pub task_id: String,
    /// Sub-agent definition id (e.g. `researcher`).
    pub agent_id: String,
    /// The sub-agent's final output / summary.
    pub summary: String,
    /// Parent chat thread id to stream the delivery turn into (captured at
    /// spawn). `None` for a headless spawn with no originating thread.
    pub parent_thread_id: Option<String>,
}

static QUEUE: OnceLock<Mutex<HashMap<String, Vec<CompletedBackgroundAgent>>>> = OnceLock::new();

fn queue() -> &'static Mutex<HashMap<String, Vec<CompletedBackgroundAgent>>> {
    QUEUE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record a finished background sub-agent for later idle delivery, keyed by
/// `parent_session`. Idempotent on `task_id` within a session.
pub fn record_completion(
    parent_session: impl Into<String>,
    task_id: impl Into<String>,
    agent_id: impl Into<String>,
    summary: impl Into<String>,
    parent_thread_id: Option<String>,
) {
    let parent_session = parent_session.into();
    let entry = CompletedBackgroundAgent {
        task_id: task_id.into(),
        agent_id: agent_id.into(),
        summary: summary.into(),
        parent_thread_id,
    };
    let mut map = queue()
        .lock()
        .expect("background_completions queue poisoned");
    let pending = map.entry(parent_session).or_default();
    if pending.iter().any(|c| c.task_id == entry.task_id) {
        return;
    }
    pending.push(entry);
}

/// Is anything waiting to be delivered for this session? Cheap idle-loop check.
pub fn has_pending(parent_session: &str) -> bool {
    queue()
        .lock()
        .expect("background_completions queue poisoned")
        .get(parent_session)
        .is_some_and(|v| !v.is_empty())
}

/// Number of results pending for a session.
pub fn pending_count(parent_session: &str) -> usize {
    queue()
        .lock()
        .expect("background_completions queue poisoned")
        .get(parent_session)
        .map_or(0, Vec::len)
}

/// Drain **all** results currently ready for this session — the "batch
/// everything ready at that moment" step. Returns them in completion order and
/// clears them so they're never re-delivered.
pub fn take_pending(parent_session: &str) -> Vec<CompletedBackgroundAgent> {
    queue()
        .lock()
        .expect("background_completions queue poisoned")
        .remove(parent_session)
        .unwrap_or_default()
}

/// The thread id to deliver a batch into — the first record that carries one.
pub fn batch_thread_id(completed: &[CompletedBackgroundAgent]) -> Option<String> {
    completed.iter().find_map(|c| c.parent_thread_id.clone())
}

/// Build the single batched, system-injected notice for a set of finished
/// background sub-agents. Each result is wrapped in a
/// `<background_agent_result id="…">` tag carrying its sub-agent process id, so
/// the agent can reference / present them individually. Returns `None` for an
/// empty batch.
pub fn build_batched_notice(completed: &[CompletedBackgroundAgent]) -> Option<String> {
    if completed.is_empty() {
        return None;
    }
    let n = completed.len();
    let mut out = String::new();
    out.push_str(&format!(
        "[{n} background sub-agent{} finished while you were busy. Review each result \
         below and present what is relevant to the user. Each is tagged with its \
         sub-agent process id.]\n",
        if n == 1 { "" } else { "s" },
    ));
    for c in completed {
        let summary = if c.summary.trim().is_empty() {
            "(no output reported)"
        } else {
            c.summary.trim()
        };
        out.push_str(&format!(
            "\n<background_agent_result id=\"{}\" agent=\"{}\">\n{}\n</background_agent_result>\n",
            c.task_id, c.agent_id, summary,
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(task: &str, agent: &str, summary: &str) -> CompletedBackgroundAgent {
        CompletedBackgroundAgent {
            task_id: task.into(),
            agent_id: agent.into(),
            summary: summary.into(),
            parent_thread_id: Some("thread-1".into()),
        }
    }

    #[test]
    fn record_and_drain_is_session_scoped_and_batches() {
        let s = "sess-batch-A";
        record_completion(s, "sub-1", "researcher", "eiffel", Some("thread-A".into()));
        record_completion(s, "sub-2", "researcher", "liberty", Some("thread-A".into()));
        record_completion("sess-other", "sub-9", "researcher", "x", None);

        assert_eq!(pending_count(s), 2);
        assert!(has_pending(s));

        let drained = take_pending(s);
        assert_eq!(
            drained
                .iter()
                .map(|c| c.task_id.as_str())
                .collect::<Vec<_>>(),
            ["sub-1", "sub-2"]
        );
        assert_eq!(batch_thread_id(&drained).as_deref(), Some("thread-A"));
        assert!(!has_pending(s));
        assert_eq!(take_pending(s), vec![]);
        assert_eq!(pending_count("sess-other"), 1);
        take_pending("sess-other");
    }

    #[test]
    fn record_is_idempotent_on_task_id() {
        let s = "sess-dupe";
        record_completion(s, "sub-1", "researcher", "first", None);
        record_completion(s, "sub-1", "researcher", "second", None);
        assert_eq!(pending_count(s), 1);
        take_pending(s);
    }

    #[test]
    fn batched_notice_tags_each_with_process_id() {
        let notice = build_batched_notice(&[
            c("sub-abc", "researcher", "Eiffel Tower: built 1889 …"),
            c("sub-def", "researcher", "Colosseum: AD 70–80 …"),
        ])
        .expect("non-empty batch");

        assert!(notice.contains("2 background sub-agents finished"));
        assert!(notice.contains("<background_agent_result id=\"sub-abc\" agent=\"researcher\">"));
        assert!(notice.contains("Eiffel Tower: built 1889"));
        assert!(notice.contains("<background_agent_result id=\"sub-def\" agent=\"researcher\">"));
        assert!(notice.contains("</background_agent_result>"));
    }

    #[test]
    fn singular_wording_and_empty_summary_fallback() {
        let notice = build_batched_notice(&[c("sub-x", "researcher", "   ")]).unwrap();
        assert!(notice.contains("1 background sub-agent finished"));
        assert!(notice.contains("(no output reported)"));
    }

    #[test]
    fn empty_batch_is_none() {
        assert_eq!(build_batched_notice(&[]), None);
    }
}
