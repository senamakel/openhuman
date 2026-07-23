//! Persistent per-thread task board used by the agent kanban UI.
//!
//! **Crate-backed.** Boards now live in the vendored `tinyagents::graph::todos`
//! crate KV store (`<workspace>/tinyagents_store/kv/graph.todos/<hex(thread_id)>`),
//! not the retired `<workspace>/agent_task_boards/<hex(thread_id)>.json`
//! file-JSON tree. [`TaskBoardStore`] is a thin adapter that preserves the
//! historical `get`/`put`/`delete` surface every consumer uses and forwards each
//! operation to the crate store via [`crate::openhuman::todos::crate_adapter`],
//! converting the crate `TaskBoard` back to the local [`TaskBoard`] and its
//! `TinyAgentsError` to a `String`.
//!
//! The agent updates boards through the `todo` tool; the UI can fetch or replace
//! them through the `threads.task_board_*` and granular `openhuman.todos_*` RPC
//! surfaces. The single-writer constraint (the core process is the only writer)
//! is documented in the adapter module.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use tinyagents::graph::todos::store as crate_todos;

use crate::openhuman::todos::crate_adapter;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCardStatus {
    Todo,
    /// Plan approval required and pending — the dispatcher parked the card here
    /// and emitted `TaskPlanAwaitingApproval`; it will not run until a human
    /// approves (→ `Ready`) or rejects (→ `Rejected`).
    AwaitingApproval,
    /// Approved for execution — the dispatcher runs `Ready` cards without a
    /// further approval check (distinguishes "approved" from the initial
    /// `Todo`, which the approval gate would otherwise re-park).
    Ready,
    InProgress,
    Blocked,
    Done,
    /// Plan approval was denied; the card is not executed.
    Rejected,
}

impl TaskCardStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskApprovalMode {
    Required,
    NotRequired,
}

impl TaskApprovalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::NotRequired => "not_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBoardCard {
    pub id: String,
    pub title: String,
    pub status: TaskCardStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<TaskApprovalMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
    /// Conversation thread id of the card's live/last agent session, when one
    /// exists. Set by the autonomous dispatcher (`task_session`) and the manual
    /// "Work" path so the UI can offer a "View session" jump into Conversations.
    /// `None` for a card that has never been run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_thread_id: Option<String>,
    /// Provider/source identifiers for a card ingested from a task source
    /// (`{provider, source_id, external_id, url, repo?, urgency}`). Set by
    /// the `task_sources` route; consumed downstream for prioritisation and
    /// external write-back. `None` for agent/UI-authored cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBoard {
    pub thread_id: String,
    pub cards: Vec<TaskBoardCard>,
    pub updated_at: String,
}

impl TaskBoard {
    pub fn empty(thread_id: impl Into<String>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            thread_id: thread_id.into(),
            cards: Vec::new(),
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskBoardStore {
    workspace_dir: PathBuf,
}

impl TaskBoardStore {
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self { workspace_dir }
    }

    /// The thread's board, or `None` when the thread has never had one. Reads
    /// the crate `graph.todos` value **raw** (no normalise) so the absent-vs-
    /// empty distinction survives, exactly as the legacy file read did.
    pub async fn get(&self, thread_id: &str) -> Result<Option<TaskBoard>, String> {
        let thread_id = validate_thread_id(thread_id)?;
        tracing::debug!(thread_id = %thread_id, "[agent:task_board] get entry");
        let store = crate_adapter::crate_todos_store(&self.workspace_dir);
        match crate_adapter::get_crate_board_raw(&store, &thread_id).await? {
            Some(crate_board) => {
                let board = crate_adapter::from_crate_board(&crate_board);
                tracing::debug!(
                    thread_id = %thread_id,
                    card_count = board.cards.len(),
                    "[agent:task_board] get ok"
                );
                Ok(Some(board))
            }
            None => {
                tracing::debug!(thread_id = %thread_id, "[agent:task_board] get not_found");
                Ok(None)
            }
        }
    }

    /// Persist `board`. Locally normalises first (minting `task-<uuid>` ids for
    /// blank cards, recomputing order), then hands the cards to the crate
    /// `replace` op — which re-normalises and **enforces the single-`InProgress`
    /// invariant** (an invalid board now errors here rather than being silently
    /// saved). Returns the saved board with crate-normalised cards.
    pub async fn put(&self, mut board: TaskBoard) -> Result<TaskBoard, String> {
        tracing::debug!(
            thread_id = %board.thread_id,
            card_count = board.cards.len(),
            "[agent:task_board] put entry"
        );
        normalise_board(&mut board);
        let thread_id = validate_thread_id(&board.thread_id)?;
        board.thread_id = thread_id.clone();
        let store = crate_adapter::crate_todos_store(&self.workspace_dir);
        let crate_cards: Vec<_> = board
            .cards
            .iter()
            .map(crate_adapter::to_crate_card)
            .collect();
        let snap = crate_todos::replace(&store, &thread_id, crate_cards)
            .await
            .map_err(|e| e.to_string())?;
        let cards: Vec<TaskBoardCard> = snap
            .cards
            .iter()
            .map(crate_adapter::from_crate_card)
            .collect();
        tracing::debug!(
            thread_id = %thread_id,
            card_count = cards.len(),
            "[agent:task_board] put ok"
        );
        Ok(TaskBoard {
            thread_id,
            cards,
            updated_at: Utc::now().to_rfc3339(),
        })
    }

    /// Delete the thread's board. Returns whether a board was present. Removes
    /// the crate key outright (not the crate `clear`, which writes an empty
    /// board) so a subsequent [`get`](Self::get) reports `None`.
    pub async fn delete(&self, thread_id: &str) -> Result<bool, String> {
        let thread_id = validate_thread_id(thread_id)?;
        tracing::debug!(thread_id = %thread_id, "[agent:task_board] delete entry");
        let store = crate_adapter::crate_todos_store(&self.workspace_dir);
        let existed = crate_adapter::get_crate_board_raw(&store, &thread_id)
            .await?
            .is_some();
        if existed {
            crate_adapter::delete_crate_board_raw(&store, &thread_id).await?;
        }
        tracing::debug!(thread_id = %thread_id, existed, "[agent:task_board] delete ok");
        Ok(existed)
    }
}

pub async fn board_for_thread(workspace_dir: &Path, thread_id: &str) -> Result<TaskBoard, String> {
    let thread_id = validate_thread_id(thread_id)?;
    let store = TaskBoardStore::new(workspace_dir.to_path_buf());
    Ok(store
        .get(&thread_id)
        .await?
        .unwrap_or_else(|| TaskBoard::empty(thread_id)))
}

pub fn normalise_board(board: &mut TaskBoard) {
    board.thread_id = board.thread_id.trim().to_string();
    let now = Utc::now().to_rfc3339();
    board.updated_at = now.clone();
    let before_count = board.cards.len();
    tracing::trace!(
        thread_id = %board.thread_id,
        card_count = before_count,
        "[agent:task_board] normalise entry"
    );

    for card in board.cards.iter_mut() {
        card.title = card.title.trim().to_string();
        if card.id.trim().is_empty() {
            card.id = format!("task-{}", uuid::Uuid::new_v4());
            tracing::trace!(
                thread_id = %board.thread_id,
                card_id = %card.id,
                "[agent:task_board] normalise generated_card_id"
            );
        } else {
            card.id = card.id.trim().to_string();
        }
        card.notes = card
            .notes
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        card.objective = card
            .objective
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        card.assigned_agent = card
            .assigned_agent
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        trim_string_vec(&mut card.plan);
        trim_string_vec(&mut card.allowed_tools);
        trim_string_vec(&mut card.acceptance_criteria);
        trim_string_vec(&mut card.evidence);
        card.blocker = card
            .blocker
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        card.session_thread_id = card
            .session_thread_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if card.status == TaskCardStatus::Blocked && card.blocker.is_none() {
            card.blocker = card.notes.clone();
            tracing::trace!(
                thread_id = %board.thread_id,
                card_id = %card.id,
                "[agent:task_board] normalise blocker_from_notes"
            );
        }
    }

    board.cards.retain(|card| !card.title.is_empty());
    let removed = before_count.saturating_sub(board.cards.len());
    if removed > 0 {
        tracing::debug!(
            thread_id = %board.thread_id,
            removed,
            "[agent:task_board] normalise removed_empty_title_cards"
        );
    }

    for (idx, card) in board.cards.iter_mut().enumerate() {
        card.order = idx as u32;
        card.updated_at = now.clone();
    }

    tracing::trace!(
        thread_id = %board.thread_id,
        card_count = board.cards.len(),
        "[agent:task_board] normalise exit"
    );
}

fn validate_thread_id(thread_id: &str) -> Result<String, String> {
    let trimmed = thread_id.trim();
    if trimmed.is_empty() {
        return Err("invalid task board thread_id: empty or whitespace".to_string());
    }
    Ok(trimmed.to_string())
}

fn trim_string_vec(values: &mut Vec<String>) {
    values.retain_mut(|value| {
        *value = value.trim().to_string();
        !value.is_empty()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn status_strings_match_serialized_statuses() {
        assert_eq!(TaskCardStatus::Todo.as_str(), "todo");
        assert_eq!(TaskCardStatus::InProgress.as_str(), "in_progress");
        assert_eq!(TaskCardStatus::Blocked.as_str(), "blocked");
        assert_eq!(TaskCardStatus::Done.as_str(), "done");
    }

    #[test]
    fn empty_board_uses_thread_id_and_no_cards() {
        let board = TaskBoard::empty("thread-empty");
        assert_eq!(board.thread_id, "thread-empty");
        assert!(board.cards.is_empty());
        assert!(!board.updated_at.is_empty());
    }

    #[tokio::test]
    async fn board_store_roundtrips_and_normalises_cards() {
        let dir = tempdir().expect("tempdir");
        let store = TaskBoardStore::new(dir.path().to_path_buf());
        let board = TaskBoard {
            thread_id: "thread-1".into(),
            cards: vec![
                TaskBoardCard {
                    id: String::new(),
                    title: "  Draft plan  ".into(),
                    status: TaskCardStatus::Todo,
                    objective: Some("  Ship task briefs  ".into()),
                    plan: vec!["  extend schema  ".into(), "  ".into()],
                    assigned_agent: Some("  planner  ".into()),
                    allowed_tools: vec![" todo ".into(), "".into()],
                    approval_mode: Some(TaskApprovalMode::Required),
                    acceptance_criteria: vec!["  tests pass  ".into()],
                    evidence: vec!["  cargo test  ".into()],
                    notes: Some("  note  ".into()),
                    blocker: None,
                    session_thread_id: Some("   ".into()),
                    source_metadata: None,
                    order: 99,
                    updated_at: String::new(),
                },
                TaskBoardCard {
                    id: "blocked".into(),
                    title: "Need approval".into(),
                    status: TaskCardStatus::Blocked,
                    objective: None,
                    plan: Vec::new(),
                    assigned_agent: None,
                    allowed_tools: Vec::new(),
                    approval_mode: None,
                    acceptance_criteria: Vec::new(),
                    evidence: Vec::new(),
                    notes: Some("waiting on user".into()),
                    blocker: None,
                    session_thread_id: None,
                    source_metadata: None,
                    order: 99,
                    updated_at: String::new(),
                },
            ],
            updated_at: String::new(),
        };

        let saved = store.put(board).await.expect("put");
        assert_eq!(saved.cards[0].title, "Draft plan");
        assert_eq!(
            saved.cards[0].objective.as_deref(),
            Some("Ship task briefs")
        );
        assert_eq!(saved.cards[0].plan, vec!["extend schema"]);
        assert_eq!(saved.cards[0].assigned_agent.as_deref(), Some("planner"));
        assert_eq!(saved.cards[0].allowed_tools, vec!["todo"]);
        assert_eq!(
            saved.cards[0].approval_mode,
            Some(TaskApprovalMode::Required)
        );
        assert_eq!(saved.cards[0].acceptance_criteria, vec!["tests pass"]);
        assert_eq!(saved.cards[0].evidence, vec!["cargo test"]);
        // Whitespace-only session_thread_id normalises to None so the board
        // never offers a blank "View session" jump target.
        assert_eq!(saved.cards[0].session_thread_id, None);
        assert_eq!(saved.cards[0].order, 0);
        assert!(saved.cards[0].id.starts_with("task-"));
        assert_eq!(saved.cards[1].blocker.as_deref(), Some("waiting on user"));

        let loaded = store.get("thread-1").await.expect("get").expect("present");
        assert_eq!(loaded.cards.len(), 2);
        assert_eq!(loaded.cards[1].status, TaskCardStatus::Blocked);

        assert!(store.delete("thread-1").await.expect("delete existing"));
        assert!(store.get("thread-1").await.expect("get deleted").is_none());
        assert!(!store.delete("thread-1").await.expect("delete missing"));
    }

    #[tokio::test]
    async fn missing_board_returns_none() {
        let dir = tempdir().expect("tempdir");
        let store = TaskBoardStore::new(dir.path().to_path_buf());
        assert!(store.get("missing").await.expect("get").is_none());
    }

    #[tokio::test]
    async fn blank_thread_id_is_rejected() {
        let dir = tempdir().expect("tempdir");
        let store = TaskBoardStore::new(dir.path().to_path_buf());
        assert!(store
            .get("   ")
            .await
            .expect_err("blank id")
            .contains("thread_id"));
    }

    #[test]
    fn normalise_recomputes_order_after_filtering_empty_titles() {
        let mut board = TaskBoard {
            thread_id: "thread-1".into(),
            cards: vec![
                TaskBoardCard {
                    id: "empty".into(),
                    title: "   ".into(),
                    status: TaskCardStatus::Todo,
                    objective: None,
                    plan: Vec::new(),
                    assigned_agent: None,
                    allowed_tools: Vec::new(),
                    approval_mode: None,
                    acceptance_criteria: Vec::new(),
                    evidence: Vec::new(),
                    notes: None,
                    blocker: None,
                    session_thread_id: None,
                    source_metadata: None,
                    order: 99,
                    updated_at: String::new(),
                },
                TaskBoardCard {
                    id: "real".into(),
                    title: "Real".into(),
                    status: TaskCardStatus::Todo,
                    objective: None,
                    plan: Vec::new(),
                    assigned_agent: None,
                    allowed_tools: Vec::new(),
                    approval_mode: None,
                    acceptance_criteria: Vec::new(),
                    evidence: Vec::new(),
                    notes: None,
                    blocker: None,
                    session_thread_id: None,
                    source_metadata: None,
                    order: 99,
                    updated_at: String::new(),
                },
            ],
            updated_at: String::new(),
        };

        normalise_board(&mut board);

        assert_eq!(board.cards.len(), 1);
        assert_eq!(board.cards[0].id, "real");
        assert_eq!(board.cards[0].order, 0);
    }

    #[tokio::test]
    async fn board_for_thread_returns_empty_board_when_file_is_missing() {
        let dir = tempdir().expect("tempdir");
        let board = board_for_thread(dir.path(), " thread-2 ")
            .await
            .expect("board");
        assert_eq!(board.thread_id, "thread-2");
        assert!(board.cards.is_empty());
    }

    /// An undecodable crate value must fail closed so a later read/modify/write
    /// operation cannot replace the authoritative row with an empty board.
    #[tokio::test]
    async fn undecodable_board_value_returns_error_without_overwriting_row() {
        use tinyagents::graph::todos::store::TODOS_NAMESPACE;

        let dir = tempdir().expect("tempdir");
        let store = crate_adapter::crate_todos_store(dir.path());
        let key = crate_adapter::todo_key("thread-corrupt");
        let undecodable = serde_json::json!({ "not": "a board" });
        store
            .put(TODOS_NAMESPACE, &key, undecodable.clone())
            .await
            .expect("seed garbage value");

        let board_store = TaskBoardStore::new(dir.path().to_path_buf());
        let error = board_store
            .get("thread-corrupt")
            .await
            .expect_err("undecodable row must not be treated as absent");
        assert!(error.contains("decode crate task board"));
        assert_eq!(
            store.get(TODOS_NAMESPACE, &key).await.expect("raw get"),
            Some(undecodable),
            "failed live reads must leave the authoritative row untouched"
        );
    }
}
