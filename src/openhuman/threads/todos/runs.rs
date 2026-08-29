//! Compatibility facade over [`tinyagents::graph::todos::runs`].
//!
//! TinyAgents owns the durable task-run record, the heartbeat, the staleness
//! policy, and the reclaim sweep (card back to `todo`, or parked at `blocked`
//! once a card has burned through its reclaim budget). What stays here is
//! OpenHuman's own shape around it: [`BoardLocation`] addressing (including the
//! process-global scratch board), RFC 3339 timestamps on the wire, the
//! `TaskRunReclaimed` domain event, and the one-time import of the retired
//! `{workspace}/agent_task_boards/<hex>.runs.json` ledger.
//!
//! Run records live in the crate KV store beside the board itself
//! (`graph.todos.runs`), so a board and its run log can no longer drift apart
//! across a restart.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tinyagents::graph::todos::runs as crate_runs;

pub use tinyagents::graph::todos::runs::{
    ReclaimDetail, ReclaimResult, RunLimits, RunOutcome, TaskRun, DEFAULT_CLAIM_TTL_SECS,
    DEFAULT_HEARTBEAT_STALE_SECS, DEFAULT_MAX_RECLAIM_COUNT,
};

use crate::openhuman::agent::task_board::normalize_timestamp_for_wire;

use super::ops::{target, BoardLocation};

/// Cadence of the background heartbeat spawned alongside an autonomous run.
const HEARTBEAT_TICK: std::time::Duration = crate_runs::DEFAULT_HEARTBEAT_TICK;

/// Legacy on-disk ledger the crate store replaced.
const TASK_BOARD_DIR: &str = "agent_task_boards";

fn map_err<T>(result: tinyagents::error::Result<T>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

/// Crate stamps are unix-epoch milliseconds; the `openhuman.todos_run_*` RPC
/// surface has always spoken RFC 3339, so translate on the way out.
fn for_wire(mut run: TaskRun) -> TaskRun {
    run.started_at = normalize_timestamp_for_wire(&run.started_at);
    run.last_heartbeat_at = normalize_timestamp_for_wire(&run.last_heartbeat_at);
    run.completed_at = run
        .completed_at
        .as_deref()
        .map(normalize_timestamp_for_wire);
    run
}

pub async fn create_run(
    location: &BoardLocation,
    run_id: &str,
    card_id: &str,
    claimed_by: &str,
) -> Result<TaskRun, String> {
    let (store, thread_id) = target(location);
    let run = map_err(
        crate_runs::create_run(&store, thread_id, Some(run_id), card_id, claimed_by).await,
    )?;
    Ok(for_wire(run))
}

pub async fn update_heartbeat(location: &BoardLocation, run_id: &str) -> Result<(), String> {
    let (store, thread_id) = target(location);
    map_err(crate_runs::update_heartbeat(&store, thread_id, run_id).await)
}

pub async fn complete_run(
    location: &BoardLocation,
    run_id: &str,
    outcome: RunOutcome,
    error: Option<String>,
    evidence: Vec<String>,
) -> Result<TaskRun, String> {
    let (store, thread_id) = target(location);
    let run = map_err(
        crate_runs::complete_run(&store, thread_id, run_id, outcome, error, evidence).await,
    )?;
    Ok(for_wire(run))
}

pub async fn list_runs(
    location: &BoardLocation,
    card_id: Option<&str>,
) -> Result<Vec<TaskRun>, String> {
    let (store, thread_id) = target(location);
    let runs = map_err(crate_runs::list_runs(&store, thread_id, card_id).await)?;
    Ok(runs.into_iter().map(for_wire).collect())
}

pub async fn get_run(location: &BoardLocation, run_id: &str) -> Result<Option<TaskRun>, String> {
    let (store, thread_id) = target(location);
    let run = map_err(crate_runs::get_run(&store, thread_id, run_id).await)?;
    Ok(run.map(for_wire))
}

pub async fn find_stale_runs(
    location: &BoardLocation,
    limits: &RunLimits,
) -> Result<Vec<(TaskRun, String)>, String> {
    let (store, thread_id) = target(location);
    let stale = map_err(crate_runs::find_stale_runs(&store, thread_id, limits).await)?;
    Ok(stale
        .into_iter()
        .map(|(run, reason)| (for_wire(run), reason))
        .collect())
}

/// Reclaim stale runs and publish a `TaskRunReclaimed` event per reclaimed
/// card, so the Tasks board UI sees a wedged card come back without a refresh.
pub async fn reclaim_stale(
    location: &BoardLocation,
    limits: &RunLimits,
) -> Result<ReclaimResult, String> {
    let (store, thread_id) = target(location);
    let result = map_err(crate_runs::reclaim_stale(&store, thread_id, limits).await)?;

    if let Some(thread_id) = location.thread_id() {
        for detail in &result.details {
            crate::core::bus::BUS.publish(crate::core::events::DomainEvent::TaskRunReclaimed {
                run_id: detail.run_id.clone(),
                card_id: detail.card_id.clone(),
                thread_id: thread_id.to_string(),
                reason: detail.reason.clone(),
            });
        }
    }
    Ok(result)
}

/// Tick the run's heartbeat in the background until it completes or `cancel`
/// fires. Board-location addressing is resolved once, here, so the crate task
/// carries only a store and a thread id.
pub fn spawn_heartbeat_task(
    location: BoardLocation,
    run_id: String,
    cancel: tokio::sync::watch::Receiver<bool>,
) {
    let (store, thread_id) = target(&location);
    crate_runs::spawn_heartbeat_task(store, thread_id.to_string(), run_id, cancel, HEARTBEAT_TICK);
}

// ── Legacy ledger migration ────────────────────────────────────────────

/// Outcome of the one-time `<hex>.runs.json` import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRunMigrationReport {
    pub total: usize,
    pub copied: usize,
    pub skipped: usize,
}

/// Copy any run ledgers left in the retired file tree into the crate store,
/// without replacing runs the crate already holds.
///
/// A thread whose crate log is non-empty is skipped wholesale: the crate log is
/// authoritative, and merging two histories would double-count the reclaims the
/// sweep's `max_reclaim_count` budget is derived from.
pub async fn migrate_legacy_task_runs(
    workspace_dir: &Path,
) -> Result<TaskRunMigrationReport, String> {
    let dir = workspace_dir.join(TASK_BOARD_DIR);
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TaskRunMigrationReport::default());
        }
        Err(error) => return Err(format!("read legacy runs dir {}: {error}", dir.display())),
    };

    let mut report = TaskRunMigrationReport::default();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("iterate legacy runs dir: {error}"))?
    {
        let path = entry.path();
        let Some(thread_id) = legacy_thread_id(&path) else {
            continue;
        };
        report.total += 1;

        let runs: Vec<TaskRun> = match tokio::fs::read_to_string(&path).await {
            Ok(body) => match serde_json::from_str(&body) {
                Ok(runs) => runs,
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "skip invalid legacy run ledger");
                    report.skipped += 1;
                    continue;
                }
            },
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skip unreadable legacy run ledger");
                report.skipped += 1;
                continue;
            }
        };

        let location = BoardLocation::Thread {
            workspace_dir: workspace_dir.to_path_buf(),
            thread_id: thread_id.clone(),
        };
        let (store, thread_id) = target(&location);
        match map_err(crate_runs::import_if_absent(&store, thread_id, runs).await) {
            Ok(true) => report.copied += 1,
            Ok(false) => report.skipped += 1,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skip legacy run ledger: store write failed");
                report.skipped += 1;
            }
        }
    }
    Ok(report)
}

/// Decode the thread id encoded in a `<hex>.runs.json` file name.
///
/// Only strict, ASCII, even-length lowercase hex is accepted. The inner two
/// bytes of every pair must both be hexadecimal digits (`0-9a-f`), so a signed
/// or malformed stem such as `+f` is rejected rather than accepted by
/// `u8::from_str_radix`. Decoding walks `hex.as_bytes()` in whole pairs, never
/// slicing a multi-byte UTF-8 character, so a non-ASCII stem like `aéb` returns
/// `None` instead of panicking mid-startup.
fn legacy_thread_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let hex = name.strip_suffix(".runs.json")?;
    if hex.is_empty() || hex.len() % 2 != 0 || !hex.is_ascii() {
        return None;
    }
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let hi = lowercase_hex_nibble(pair[0])?;
            let lo = lowercase_hex_nibble(pair[1])?;
            Some(hi * 16 + lo)
        })
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

/// Decode one ASCII byte as a lowercase hexadecimal nibble (`0-9a-f`), or
/// `None` for any other byte (including uppercase `A-F`).
fn lowercase_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::agent::task_board::TaskCardStatus;
    use crate::openhuman::threads::todos::ops::{self, CardPatch};
    use tempfile::tempdir;

    fn thread_loc(dir: &Path, id: &str) -> BoardLocation {
        BoardLocation::Thread {
            workspace_dir: dir.to_path_buf(),
            thread_id: id.to_string(),
        }
    }

    /// Lowercase-hex encode a thread id, matching [`super::legacy_thread_id`]'s
    /// decoder so test-built keys round-trip through the migration.
    fn hex_key(id: &str) -> String {
        id.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    #[tokio::test]
    async fn create_and_list_run() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "run-test-1");

        let run = create_run(&loc, "run-1", "card-1", "default")
            .await
            .unwrap();
        assert_eq!(run.run_id, "run-1");
        assert_eq!(run.card_id, "card-1");
        assert_eq!(run.claimed_by, "default");
        assert!(run.is_active());
        assert!(!run.claim_token.is_empty());

        let all = list_runs(&loc, None).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(list_runs(&loc, Some("card-1")).await.unwrap().len(), 1);
        assert!(list_runs(&loc, Some("card-other"))
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn timestamps_reach_the_wire_as_rfc3339() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "wire-test");
        let run = create_run(&loc, "run-1", "card-1", "default")
            .await
            .unwrap();

        // The RPC surface has always spoken RFC 3339; the crate stores millis.
        assert!(chrono::DateTime::parse_from_rfc3339(&run.started_at).is_ok());
        assert!(chrono::DateTime::parse_from_rfc3339(&run.last_heartbeat_at).is_ok());

        let done = complete_run(&loc, "run-1", RunOutcome::Success, None, vec![])
            .await
            .unwrap();
        let completed_at = done.completed_at.expect("completed stamp");
        assert!(chrono::DateTime::parse_from_rfc3339(&completed_at).is_ok());
    }

    #[tokio::test]
    async fn heartbeat_updates_timestamp() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "hb-test-1");

        create_run(&loc, "run-hb", "card-1", "default")
            .await
            .unwrap();
        let before = get_run(&loc, "run-hb").await.unwrap().unwrap();

        update_heartbeat(&loc, "run-hb").await.unwrap();

        let after = get_run(&loc, "run-hb").await.unwrap().unwrap();
        assert!(after.last_heartbeat_at >= before.last_heartbeat_at);
    }

    #[tokio::test]
    async fn heartbeat_fails_for_completed_run() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "hb-test-2");

        create_run(&loc, "run-done", "card-1", "default")
            .await
            .unwrap();
        complete_run(&loc, "run-done", RunOutcome::Success, None, vec![])
            .await
            .unwrap();

        assert!(update_heartbeat(&loc, "run-done").await.is_err());
    }

    #[tokio::test]
    async fn complete_run_sets_outcome() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "complete-test");

        create_run(&loc, "run-ok", "card-1", "default")
            .await
            .unwrap();
        let done = complete_run(
            &loc,
            "run-ok",
            RunOutcome::Success,
            None,
            vec!["evidence".to_string()],
        )
        .await
        .unwrap();

        assert!(!done.is_active());
        assert_eq!(done.outcome, Some(RunOutcome::Success));
        assert_eq!(done.evidence, vec!["evidence".to_string()]);
    }

    #[tokio::test]
    async fn complete_run_with_failure() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "fail-test");

        create_run(&loc, "run-bad", "card-1", "default")
            .await
            .unwrap();
        let done = complete_run(
            &loc,
            "run-bad",
            RunOutcome::Failed,
            Some("boom".to_string()),
            vec![],
        )
        .await
        .unwrap();

        assert_eq!(done.outcome, Some(RunOutcome::Failed));
        assert_eq!(done.error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn get_run_returns_none_for_missing() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "missing-test");
        assert!(get_run(&loc, "nope").await.unwrap().is_none());
    }

    /// Age a run past every limit by rewriting its stamps in the crate store.
    async fn wedge(loc: &BoardLocation, run_id: &str) {
        let (store, thread_id) = target(loc);
        let mut runs = crate_runs::list_runs(&store, thread_id, None)
            .await
            .unwrap();
        for run in runs.iter_mut().filter(|run| run.run_id == run_id) {
            run.started_at = "0".to_string();
            run.last_heartbeat_at = "0".to_string();
        }
        let key = hex_key(&thread_id);
        store
            .put(
                crate_runs::RUNS_NAMESPACE,
                &key,
                serde_json::to_value(&runs).unwrap(),
            )
            .await
            .unwrap();
    }

    async fn seed_in_progress_card(loc: &BoardLocation, title: &str) -> String {
        let snapshot = ops::add(loc, title, CardPatch::default()).await.unwrap();
        let card_id = snapshot.cards.last().unwrap().id.clone();
        ops::edit(
            loc,
            &card_id,
            CardPatch {
                status: Some(TaskCardStatus::InProgress),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        card_id
    }

    #[tokio::test]
    async fn reclaim_stale_moves_card_to_todo() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "reclaim-test");
        let card_id = seed_in_progress_card(&loc, "wedged work").await;

        create_run(&loc, "run-stale", &card_id, "default")
            .await
            .unwrap();
        wedge(&loc, "run-stale").await;

        let result = reclaim_stale(&loc, &RunLimits::default()).await.unwrap();
        assert_eq!(result.reclaimed_count, 1);
        assert_eq!(result.blocked_count, 0);
        assert_eq!(result.details[0].new_card_status, "todo");

        let snapshot = ops::list(&loc).await.unwrap();
        assert_eq!(snapshot.cards[0].status, TaskCardStatus::Todo);
    }

    #[tokio::test]
    async fn reclaim_blocks_after_max_reclaims() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "reclaim-max-test");
        let card_id = seed_in_progress_card(&loc, "poison work").await;
        // Two reclaims are tolerated; the one that reaches the limit parks it.
        let limits = RunLimits {
            max_reclaim_count: 2,
            ..RunLimits::default()
        };

        for (attempt, expected) in ["todo", "blocked"].iter().enumerate() {
            if attempt > 0 {
                ops::edit(
                    &loc,
                    &card_id,
                    CardPatch {
                        status: Some(TaskCardStatus::InProgress),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            }
            let run_id = format!("run-{attempt}");
            create_run(&loc, &run_id, &card_id, "default")
                .await
                .unwrap();
            wedge(&loc, &run_id).await;
            let result = reclaim_stale(&loc, &limits).await.unwrap();
            assert_eq!(&result.details[0].new_card_status, expected);
        }

        let snapshot = ops::list(&loc).await.unwrap();
        assert_eq!(snapshot.cards[0].status, TaskCardStatus::Blocked);
        assert!(snapshot.cards[0]
            .blocker
            .as_deref()
            .unwrap_or_default()
            .contains("exceeding limit of 2"));
    }

    #[tokio::test]
    async fn reclaim_skips_healthy_runs() {
        let dir = tempdir().unwrap();
        let loc = thread_loc(dir.path(), "reclaim-healthy-test");
        let card_id = seed_in_progress_card(&loc, "live work").await;
        create_run(&loc, "run-live", &card_id, "default")
            .await
            .unwrap();

        let result = reclaim_stale(&loc, &RunLimits::default()).await.unwrap();
        assert_eq!(result.reclaimed_count, 0);
        assert_eq!(result.blocked_count, 0);

        let snapshot = ops::list(&loc).await.unwrap();
        assert_eq!(snapshot.cards[0].status, TaskCardStatus::InProgress);
    }

    #[tokio::test]
    async fn scratch_location_returns_empty_runs() {
        // Serialize against the process-global scratch store shared with
        // `todos::ops` / agent-tool tests (see `scratch_test_lock`).
        let _guard = ops::scratch_test_lock();
        let runs = list_runs(&BoardLocation::Scratch, None).await.unwrap();
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn legacy_ledger_is_imported_once_and_never_replaces_crate_runs() {
        let workspace = tempdir().unwrap();
        let legacy_dir = workspace.path().join(TASK_BOARD_DIR);
        tokio::fs::create_dir_all(&legacy_dir).await.unwrap();

        let thread_id = "legacy-thread";
        let hex = hex_key(thread_id);
        let legacy = vec![TaskRun {
            run_id: "legacy-run".to_string(),
            card_id: "card-1".to_string(),
            claimed_by: "default".to_string(),
            claim_token: "token".to_string(),
            started_at: "0".to_string(),
            last_heartbeat_at: "0".to_string(),
            completed_at: None,
            outcome: None,
            error: None,
            evidence: Vec::new(),
        }];
        tokio::fs::write(
            legacy_dir.join(format!("{hex}.runs.json")),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .await
        .unwrap();
        // A board file alongside it must not be mistaken for a run ledger.
        tokio::fs::write(legacy_dir.join(format!("{hex}.json")), b"{}")
            .await
            .unwrap();

        let first = migrate_legacy_task_runs(workspace.path()).await.unwrap();
        assert_eq!(
            first,
            TaskRunMigrationReport {
                total: 1,
                copied: 1,
                skipped: 0,
            }
        );

        let loc = thread_loc(workspace.path(), thread_id);
        assert_eq!(list_runs(&loc, None).await.unwrap().len(), 1);

        // Second pass: the crate log is authoritative and is left alone.
        let second = migrate_legacy_task_runs(workspace.path()).await.unwrap();
        assert_eq!(second.copied, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(list_runs(&loc, None).await.unwrap().len(), 1);
    }

    #[test]
    fn legacy_file_names_decode_only_run_ledgers() {
        let hex: String = "thread-1"
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            legacy_thread_id(Path::new(&format!("/w/{hex}.runs.json"))).as_deref(),
            Some("thread-1")
        );
        assert!(legacy_thread_id(Path::new(&format!("/w/{hex}.json"))).is_none());
        assert!(legacy_thread_id(Path::new("/w/notes.txt")).is_none());
        assert!(legacy_thread_id(Path::new("/w/zz.runs.json")).is_none());
    }

    #[test]
    fn legacy_file_names_reject_malformed_stems_without_panicking() {
        // Multi-byte UTF-8 in the stem slices an odd index if decoded by byte
        // pairs; it must be rejected, not panic.
        assert!(legacy_thread_id(Path::new("/w/aéb.runs.json")).is_none());
        // from_str_radix would accept "+f" as 15; strict hex decoding rejects the sign.
        assert!(legacy_thread_id(Path::new("/w/+f+f.runs.json")).is_none());
        assert!(legacy_thread_id(Path::new("/w/gg.runs.json")).is_none());
        assert!(legacy_thread_id(Path::new("/w/GG.runs.json")).is_none());
        // Uppercase hex is rejected even though to_digit(16) would accept it.
        assert!(legacy_thread_id(Path::new("/w/4A.runs.json")).is_none());
        assert!(legacy_thread_id(Path::new("/w/4a.runs.json")).is_some());
        // A mixed pair with a valid second nibble but invalid first is rejected.
        assert!(legacy_thread_id(Path::new("/w/0g.runs.json")).is_none());
        // Non-hex ASCII letters are rejected.
        assert!(legacy_thread_id(Path::new("/w/zz.runs.json")).is_none());
    }
}
