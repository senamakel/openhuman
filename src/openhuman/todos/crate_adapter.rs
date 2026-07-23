//! Adapter seam onto the vendored `tinyagents::graph::todos` crate store.
//!
//! The crate store is now **authoritative** for per-thread task boards — the
//! OpenHuman [`TaskBoardStore`](crate::openhuman::agent::task_board::TaskBoardStore)
//! and [`todos::ops`](crate::openhuman::todos::ops) `Thread` path delegate every
//! read/write to it. This module supplies the conversion helpers (OpenHuman ↔
//! crate `TaskBoardCard`/`TaskBoard`/`TaskCardStatus`/`TaskApprovalMode`), the
//! store-handle opener ([`crate_todos_store`]), the raw crate-board read/write
//! helpers used for the existence-preserving `get`/`delete` semantics, and the
//! idempotent [`migrate_legacy_task_boards_into_crate_store`] boot helper that
//! copies boards left in the retired `<workspace>/agent_task_boards/`
//! file-JSON tree only when the crate store has no value for that thread.
//!
//! Persistence target: the crate [`Store`] rooted at the shared workspace KV
//! tree (`<workspace>/tinyagents_store/kv`), namespace [`TODOS_NAMESPACE`]
//! (`graph.todos`), keyed by `hex(thread_id)` — byte-for-byte the key the
//! crate's own `graph::todos::store` computes.
//!
//! # Scratch has no crate target
//! OpenHuman keeps an in-memory, thread-less
//! [`BoardLocation::Scratch`](crate::openhuman::todos::ops::BoardLocation)
//! fallback (tool calls outside a chat thread). The crate board is always
//! `(Store, thread_id)`, so scratch stays entirely on the local
//! [`ScratchTodoStore`](crate::openhuman::todos::store::ScratchTodoStore) path;
//! only `BoardLocation::Thread` routes here.
//!
//! # Single-writer constraint
//! The crate `Store` has no cross-process compare-and-set; its per-thread
//! atomicity is a process-local async mutex. This is acceptable because the
//! OpenHuman core is the single writer of task boards.

use std::path::Path;
use std::sync::Arc;

use tinyagents::graph::todos::store::TODOS_NAMESPACE;
use tinyagents::graph::todos::{
    TaskApprovalMode as CrateApprovalMode, TaskBoard as CrateBoard, TaskBoardCard as CrateCard,
    TaskCardStatus as CrateStatus,
};
use tinyagents::harness::store::Store;

use crate::openhuman::agent::task_board::{
    TaskApprovalMode as OhApprovalMode, TaskBoard as OhBoard, TaskBoardCard as OhCard,
    TaskCardStatus as OhStatus,
};
use crate::openhuman::session_import::ops::open_session_stores;

/// Open the crate [`Store`] handle backing the `graph.todos` namespace, rooted
/// at the shared workspace KV tree (`<workspace>/tinyagents_store/kv`). Same
/// layout the 04-sessions journal + thread goals use, so everything lives under
/// one tree.
pub(crate) fn crate_todos_store(workspace_dir: &Path) -> Arc<dyn Store> {
    Arc::new(open_session_stores(workspace_dir).kv)
}

/// The crate store key for a thread's board: lowercase hex of the (trimmed)
/// thread-id bytes. MUST match the crate's private `graph::todos::store` key
/// function exactly so the crate reader resolves our value.
pub(crate) fn todo_key(thread_id: &str) -> String {
    thread_id
        .trim()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Maps an OpenHuman [`OhStatus`] to the crate [`CrateStatus`]. Total (the two
/// enums share the same seven variants).
pub(crate) fn map_status_to_crate(status: &OhStatus) -> CrateStatus {
    match status {
        OhStatus::Todo => CrateStatus::Todo,
        OhStatus::AwaitingApproval => CrateStatus::AwaitingApproval,
        OhStatus::Ready => CrateStatus::Ready,
        OhStatus::InProgress => CrateStatus::InProgress,
        OhStatus::Blocked => CrateStatus::Blocked,
        OhStatus::Done => CrateStatus::Done,
        OhStatus::Rejected => CrateStatus::Rejected,
    }
}

/// Maps a crate [`CrateStatus`] back to an OpenHuman [`OhStatus`]. Total; the
/// inverse of [`map_status_to_crate`].
pub(crate) fn map_status_from_crate(status: CrateStatus) -> OhStatus {
    match status {
        CrateStatus::Todo => OhStatus::Todo,
        CrateStatus::AwaitingApproval => OhStatus::AwaitingApproval,
        CrateStatus::Ready => OhStatus::Ready,
        CrateStatus::InProgress => OhStatus::InProgress,
        CrateStatus::Blocked => OhStatus::Blocked,
        CrateStatus::Done => OhStatus::Done,
        CrateStatus::Rejected => OhStatus::Rejected,
    }
}

fn map_approval_mode(mode: &OhApprovalMode) -> CrateApprovalMode {
    match mode {
        OhApprovalMode::Required => CrateApprovalMode::Required,
        OhApprovalMode::NotRequired => CrateApprovalMode::NotRequired,
    }
}

fn map_approval_mode_from_crate(mode: CrateApprovalMode) -> OhApprovalMode {
    match mode {
        CrateApprovalMode::Required => OhApprovalMode::Required,
        CrateApprovalMode::NotRequired => OhApprovalMode::NotRequired,
    }
}

/// Converts an OpenHuman [`OhCard`] into the crate [`CrateCard`], preserving the
/// id, status, and all optional metadata so a persisted board round-trips.
pub(crate) fn to_crate_card(card: &OhCard) -> CrateCard {
    CrateCard {
        id: card.id.clone(),
        title: card.title.clone(),
        status: map_status_to_crate(&card.status),
        objective: card.objective.clone(),
        plan: card.plan.clone(),
        assigned_agent: card.assigned_agent.clone(),
        allowed_tools: card.allowed_tools.clone(),
        approval_mode: card.approval_mode.as_ref().map(map_approval_mode),
        acceptance_criteria: card.acceptance_criteria.clone(),
        evidence: card.evidence.clone(),
        notes: card.notes.clone(),
        blocker: card.blocker.clone(),
        session_thread_id: card.session_thread_id.clone(),
        source_metadata: card.source_metadata.clone(),
        order: card.order,
        updated_at: card.updated_at.clone(),
    }
}

/// Converts a crate [`CrateCard`] back into an OpenHuman [`OhCard`] (the inverse
/// of [`to_crate_card`]), used by the read path to return local cards from the
/// crate store.
pub(crate) fn from_crate_card(card: &CrateCard) -> OhCard {
    OhCard {
        id: card.id.clone(),
        title: card.title.clone(),
        status: map_status_from_crate(card.status),
        objective: card.objective.clone(),
        plan: card.plan.clone(),
        assigned_agent: card.assigned_agent.clone(),
        allowed_tools: card.allowed_tools.clone(),
        approval_mode: card.approval_mode.map(map_approval_mode_from_crate),
        acceptance_criteria: card.acceptance_criteria.clone(),
        evidence: card.evidence.clone(),
        notes: card.notes.clone(),
        blocker: card.blocker.clone(),
        session_thread_id: card.session_thread_id.clone(),
        source_metadata: card.source_metadata.clone(),
        order: card.order,
        updated_at: card.updated_at.clone(),
    }
}

/// Convert a whole OpenHuman [`OhBoard`] into a crate [`CrateBoard`] (faithful
/// projection — no re-mint, no re-normalise).
pub(crate) fn to_crate_board(board: &OhBoard) -> CrateBoard {
    CrateBoard {
        thread_id: board.thread_id.clone(),
        cards: board.cards.iter().map(to_crate_card).collect(),
        updated_at: board.updated_at.clone(),
    }
}

/// Convert a crate [`CrateBoard`] back into an OpenHuman [`OhBoard`].
pub(crate) fn from_crate_board(board: &CrateBoard) -> OhBoard {
    OhBoard {
        thread_id: board.thread_id.clone(),
        cards: board.cards.iter().map(from_crate_card).collect(),
        updated_at: board.updated_at.clone(),
    }
}

/// Read the raw crate [`CrateBoard`] stored for `thread_id` (ns `graph.todos`,
/// key `hex(thread_id)`), or `None` when the thread has no board value. Does
/// **not** normalise. An undecodable value is treated as absent.
pub(crate) async fn get_crate_board_raw(
    store: &Arc<dyn Store>,
    thread_id: &str,
) -> Result<Option<CrateBoard>, String> {
    let value = store
        .get(TODOS_NAMESPACE, &todo_key(thread_id))
        .await
        .map_err(|e| format!("read crate task board: {e}"))?;
    match value {
        Some(v) => match serde_json::from_value::<CrateBoard>(v) {
            Ok(board) => Ok(Some(board)),
            Err(e) => {
                tracing::debug!(
                    thread_id = %thread_id,
                    error = %e,
                    "[todos][crate-adapter] undecodable crate board; treating as absent"
                );
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// Whether the crate store contains any value for `thread_id`, without
/// attempting to decode it. Migration uses this stricter existence check so a
/// corrupt or forward-schema value remains authoritative and is never
/// overwritten by a stale legacy file.
async fn crate_board_value_exists(store: &Arc<dyn Store>, thread_id: &str) -> Result<bool, String> {
    store
        .get(TODOS_NAMESPACE, &todo_key(thread_id))
        .await
        .map(|value| value.is_some())
        .map_err(|e| format!("check crate task board existence: {e}"))
}

/// Delete the crate board value for `thread_id` (ns `graph.todos`). No-op when
/// absent. Unlike the crate `clear` op (which writes an empty board), this
/// removes the key so a subsequent read reports `None` — matching the legacy
/// file-delete semantics.
pub(crate) async fn delete_crate_board_raw(
    store: &Arc<dyn Store>,
    thread_id: &str,
) -> Result<(), String> {
    store
        .delete(TODOS_NAMESPACE, &todo_key(thread_id))
        .await
        .map_err(|e| format!("delete crate task board: {e}"))
}

/// Write a faithful raw crate mirror of `board` (ns `graph.todos`). Used only by
/// the boot migration; the live path goes through the crate `replace` op.
async fn put_crate_board_raw(store: &Arc<dyn Store>, board: &CrateBoard) -> Result<(), String> {
    let value = serde_json::to_value(board).map_err(|e| format!("serialize crate board: {e}"))?;
    store
        .put(TODOS_NAMESPACE, &todo_key(&board.thread_id), value)
        .await
        .map_err(|e| format!("mirror task board into {TODOS_NAMESPACE}: {e}"))
}

// ── Idempotent legacy→crate migration helper (run on each core boot) ──────────

/// Outcome of a [`migrate_legacy_task_boards_into_crate_store`] run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskBoardMigrationReport {
    /// Legacy board files examined.
    pub total: usize,
    /// Boards written into the crate store because no crate value existed.
    pub copied: usize,
    /// Boards already present in the crate store and left authoritative.
    pub skipped: usize,
}

/// Read any boards left in the retired legacy `<workspace>/agent_task_boards/`
/// file-JSON tree. Returns an empty vec when the directory is absent (the common
/// case after the first migration). The per-thread run ledger (`*.runs.json`)
/// stays local and is skipped. Undecodable/unreadable files are skipped so a
/// stray file can't wedge the idempotent copy.
async fn read_legacy_file_boards(workspace_dir: &Path) -> Result<Vec<OhBoard>, String> {
    const LEGACY_DIR: &str = "agent_task_boards";
    const LEGACY_EXT: &str = "json";
    const RUNS_SUFFIX: &str = ".runs.json";
    let dir = workspace_dir.join(LEGACY_DIR);
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(format!(
                "read legacy task boards dir {}: {e}",
                dir.display()
            ))
        }
    };
    let mut boards = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("iterate legacy task boards dir: {e}"))?
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(LEGACY_EXT) {
            continue;
        }
        // Skip the run ledger sidecar files, which stay on the local fs path.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(RUNS_SUFFIX))
            .unwrap_or(false)
        {
            continue;
        }
        match tokio::fs::read_to_string(&path).await {
            Ok(body) => match serde_json::from_str::<OhBoard>(&body) {
                Ok(board) => boards.push(board),
                Err(e) => {
                    tracing::debug!(path = %path.display(), error = %e, "[todos][crate-migrate] skip parse error");
                }
            },
            Err(e) => {
                tracing::debug!(path = %path.display(), error = %e, "[todos][crate-migrate] skip read error");
            }
        }
    }
    Ok(boards)
}

/// Copy legacy task boards missing from the crate `graph.todos` store.
///
/// **Idempotent and non-destructive**: any existing crate value is
/// authoritative and skipped, even when it differs from the stale legacy
/// file. This prevents a later process restart from reverting post-migration
/// edits while the retired files remain on disk.
///
/// Wired into `core::runtime::services` on every core boot. Honors the
/// single-writer constraint: run it only inside the core process.
pub async fn migrate_legacy_task_boards_into_crate_store(
    workspace_dir: &Path,
) -> Result<TaskBoardMigrationReport, String> {
    let legacy = read_legacy_file_boards(workspace_dir).await?;
    let store = crate_todos_store(workspace_dir);
    let mut report = TaskBoardMigrationReport {
        total: legacy.len(),
        ..Default::default()
    };
    tracing::info!(
        workspace = %workspace_dir.display(),
        total = report.total,
        "[todos][crate-migrate] start copy legacy task boards → graph.todos"
    );
    for board in &legacy {
        let crate_board = to_crate_board(board);
        match crate_board_value_exists(&store, &board.thread_id).await {
            Ok(true) => {
                report.skipped += 1;
                tracing::debug!(
                    thread_id = %board.thread_id,
                    "[todos][crate-migrate] skip (crate board already authoritative)"
                );
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                return Err(format!(
                    "check crate task board before migrating thread {}: {e}",
                    board.thread_id
                ))
            }
        }
        put_crate_board_raw(&store, &crate_board).await?;
        report.copied += 1;
        tracing::debug!(
            thread_id = %board.thread_id,
            card_count = crate_board.cards.len(),
            "[todos][crate-migrate] copied"
        );
    }
    tracing::info!(
        total = report.total,
        copied = report.copied,
        skipped = report.skipped,
        "[todos][crate-migrate] done"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oh_card(id: &str, status: OhStatus) -> OhCard {
        OhCard {
            id: id.to_string(),
            title: format!("card {id}"),
            status,
            objective: Some("obj".to_string()),
            plan: vec!["step-1".to_string()],
            assigned_agent: Some("planner".to_string()),
            allowed_tools: vec!["todo".to_string()],
            approval_mode: Some(OhApprovalMode::Required),
            acceptance_criteria: vec!["tests pass".to_string()],
            evidence: vec!["cargo test".to_string()],
            notes: Some("note".to_string()),
            blocker: None,
            session_thread_id: Some("thread-x".to_string()),
            source_metadata: Some(serde_json::json!({ "urgency": 0.5 })),
            order: 3,
            updated_at: "2026-07-03T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn status_mapping_is_total_and_round_trips() {
        let all = [
            OhStatus::Todo,
            OhStatus::AwaitingApproval,
            OhStatus::Ready,
            OhStatus::InProgress,
            OhStatus::Blocked,
            OhStatus::Done,
            OhStatus::Rejected,
        ];
        for oh in all {
            let crate_status = map_status_to_crate(&oh);
            assert_eq!(oh.as_str(), crate_status.as_str());
            assert_eq!(map_status_from_crate(crate_status), oh);
        }
    }

    #[test]
    fn card_conversion_round_trips_all_fields() {
        let oh = oh_card("task-1", OhStatus::InProgress);
        let c = to_crate_card(&oh);
        assert_eq!(c.id, "task-1");
        assert_eq!(c.status, CrateStatus::InProgress);
        assert_eq!(c.approval_mode, Some(CrateApprovalMode::Required));
        assert_eq!(c.updated_at, "2026-07-03T00:00:00Z");
        // Full round-trip identity.
        assert_eq!(from_crate_card(&c), oh);
    }

    #[test]
    fn approval_mode_maps_both_directions() {
        assert_eq!(
            map_approval_mode(&OhApprovalMode::Required),
            CrateApprovalMode::Required
        );
        assert_eq!(
            map_approval_mode_from_crate(CrateApprovalMode::NotRequired),
            OhApprovalMode::NotRequired
        );
    }

    #[tokio::test]
    async fn raw_get_delete_round_trip_and_crate_reader_agrees() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = crate_todos_store(dir.path());
        let board = OhBoard {
            thread_id: "user-tasks".to_string(),
            cards: vec![
                oh_card("task-a", OhStatus::Todo),
                oh_card("task-b", OhStatus::InProgress),
            ],
            updated_at: "2026-07-03T00:00:00Z".to_string(),
        };
        assert!(get_crate_board_raw(&store, "user-tasks")
            .await
            .unwrap()
            .is_none());
        put_crate_board_raw(&store, &to_crate_board(&board))
            .await
            .unwrap();
        let read = get_crate_board_raw(&store, "user-tasks")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(from_crate_board(&read), board, "raw board round-trips");

        // The crate's own reader resolves the same key/namespace we wrote.
        let via_crate = tinyagents::graph::todos::store::list(&store, "user-tasks")
            .await
            .unwrap();
        assert_eq!(via_crate.cards.len(), 2);
        assert_eq!(via_crate.cards[1].status, CrateStatus::InProgress);

        delete_crate_board_raw(&store, "user-tasks").await.unwrap();
        assert!(get_crate_board_raw(&store, "user-tasks")
            .await
            .unwrap()
            .is_none());
        // Delete is idempotent (no-op when absent).
        delete_crate_board_raw(&store, "user-tasks").await.unwrap();
    }

    /// Write a board into the retired legacy `<workspace>/agent_task_boards/`
    /// file-JSON tree (the shape `read_legacy_file_boards` reads).
    fn write_legacy_board(dir: &std::path::Path, board: &OhBoard) {
        let legacy_dir = dir.join("agent_task_boards");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let path = legacy_dir.join(format!("{}.json", hex::encode(board.thread_id.as_bytes())));
        std::fs::write(&path, serde_json::to_string(board).unwrap()).unwrap();
    }

    fn legacy_board(thread_id: &str, card_id: &str) -> OhBoard {
        OhBoard {
            thread_id: thread_id.to_string(),
            cards: vec![oh_card(card_id, OhStatus::Todo)],
            updated_at: "2026-07-03T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn migration_copies_then_is_idempotent_and_skips_runs_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_legacy_board(dir, &legacy_board("t1", "task-1"));
        write_legacy_board(dir, &legacy_board("t2", "task-2"));
        // A runs sidecar must be ignored, not parsed as a board.
        let legacy_dir = dir.join("agent_task_boards");
        std::fs::write(
            legacy_dir.join(format!("{}.runs.json", hex::encode("t1".as_bytes()))),
            "[]",
        )
        .unwrap();

        let r1 = migrate_legacy_task_boards_into_crate_store(dir)
            .await
            .unwrap();
        assert_eq!(r1.total, 2, "runs sidecar not counted as a board");
        assert_eq!(r1.copied, 2);
        assert_eq!(r1.skipped, 0);

        let store = crate_todos_store(dir);
        let m2 = get_crate_board_raw(&store, "t2").await.unwrap().unwrap();
        assert_eq!(m2.cards[0].id, "task-2");

        // Simulate a live post-migration edit while the legacy files remain.
        let mut edited = m2;
        edited.cards[0].title = "newer crate title".to_string();
        edited.updated_at = "2026-07-04T00:00:00Z".to_string();
        put_crate_board_raw(&store, &edited).await.unwrap();

        // Second run is a no-op (idempotent) and must not restore stale data.
        let r2 = migrate_legacy_task_boards_into_crate_store(dir)
            .await
            .unwrap();
        assert_eq!(r2.total, 2);
        assert_eq!(r2.copied, 0, "idempotent: nothing re-copied");
        assert_eq!(r2.skipped, 2);
        let preserved = get_crate_board_raw(&store, "t2").await.unwrap().unwrap();
        assert_eq!(preserved.cards[0].title, "newer crate title");
        assert_eq!(preserved.updated_at, "2026-07-04T00:00:00Z");
    }

    #[tokio::test]
    async fn migration_preserves_undecodable_but_present_crate_board() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write_legacy_board(dir, &legacy_board("t-corrupt", "stale-task"));

        let store = crate_todos_store(dir);
        let key = todo_key("t-corrupt");
        let undecodable = serde_json::json!({
            "thread_id": "t-corrupt",
            "cards": "future-schema",
        });
        store
            .put(TODOS_NAMESPACE, &key, undecodable.clone())
            .await
            .unwrap();

        let report = migrate_legacy_task_boards_into_crate_store(dir)
            .await
            .unwrap();
        assert_eq!(report.total, 1);
        assert_eq!(report.copied, 0);
        assert_eq!(report.skipped, 1);
        assert_eq!(
            store.get(TODOS_NAMESPACE, &key).await.unwrap(),
            Some(undecodable),
            "migration must not overwrite a present row it cannot decode"
        );
    }
}
