//! Adapter seam onto the tinyagents `graph::goals` crate store (issue #4249).
//!
//! The crate store is now **authoritative** for thread goals — [`super::store`]
//! delegates every operation to it. This module supplies the conversion helpers
//! (local ↔ crate [`ThreadGoal`]/[`ThreadGoalStatus`]), the store-handle opener
//! ([`crate_goals_store`]), the raw mirror read/write/delete helpers used by the
//! one operation without a crate equivalent (the unconditional
//! `set_continuation_suppressed`), and the one-time
//! [`migrate_legacy_goals_into_crate_store`] boot helper that copies goals left
//! in the retired `{workspace}/thread_goals/` file-JSON tree only when the crate
//! store has no value for that thread.
//!
//! Persistence target: the crate [`Store`] rooted at the shared workspace KV
//! tree (`{workspace}/tinyagents_store/kv`), namespace [`GOALS_NAMESPACE`]
//! (`graph.goals`), keyed by `hex(thread_id)` — byte-for-byte the key the
//! crate's own `graph::goals::store` computes.
//!
//! # Single-writer constraint
//!
//! The crate `Store` has **no compare-and-set and no cross-key transaction**;
//! its per-thread atomicity is a *process-local* async mutex. This is acceptable
//! because **the OpenHuman core is the single writer** of thread goals — RPC
//! handlers, agent tools, and the heartbeat continuation runtime all run inside
//! one core process. Do not add a second mutating writer (a sidecar, a second
//! core, a cron in another process) without introducing a real CAS first.

use std::path::Path;
use std::sync::Arc;

use tinyagents::graph::goals::store::GOALS_NAMESPACE;
use tinyagents::graph::goals::{ThreadGoal as CrateThreadGoal, ThreadGoalStatus as CrateStatus};
use tinyagents::harness::store::Store;

use super::types::{ThreadGoal, ThreadGoalStatus};
use crate::openhuman::session_import::ops::open_session_stores;

/// Open the crate [`Store`] handle used for the goals mirror, rooted at the
/// shared workspace KV tree (`{workspace}/tinyagents_store/kv`). Same layout the
/// 04-sessions journal + status store use, so everything lives under one tree.
pub(crate) fn crate_goals_store(workspace_dir: &Path) -> Arc<dyn Store> {
    Arc::new(open_session_stores(workspace_dir).kv)
}

/// The crate store key for a thread's goal: lowercase hex of the (trimmed)
/// thread-id bytes. This MUST match the crate's private `graph::goals::store`
/// key function exactly so the crate reader resolves our mirrored value.
fn goal_key(thread_id: &str) -> String {
    thread_id
        .trim()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Map a legacy [`ThreadGoalStatus`] onto the crate [`CrateStatus`]. The two
/// enums are 1:1 (Active/Paused/BudgetLimited/Complete) — this is the mapping
/// the parity tests pin.
pub(crate) fn to_crate_status(status: ThreadGoalStatus) -> CrateStatus {
    match status {
        ThreadGoalStatus::Active => CrateStatus::Active,
        ThreadGoalStatus::Paused => CrateStatus::Paused,
        ThreadGoalStatus::BudgetLimited => CrateStatus::BudgetLimited,
        ThreadGoalStatus::Complete => CrateStatus::Complete,
    }
}

/// Map a crate [`CrateStatus`] back onto the legacy [`ThreadGoalStatus`] (the
/// inverse of [`to_crate_status`]).
pub(crate) fn from_crate_status(status: CrateStatus) -> ThreadGoalStatus {
    match status {
        CrateStatus::Active => ThreadGoalStatus::Active,
        CrateStatus::Paused => ThreadGoalStatus::Paused,
        CrateStatus::BudgetLimited => ThreadGoalStatus::BudgetLimited,
        CrateStatus::Complete => ThreadGoalStatus::Complete,
    }
}

/// Convert a legacy [`ThreadGoal`] into the crate [`CrateThreadGoal`],
/// preserving every field verbatim (id, objective, status, budget/usage
/// counters, timestamps, continuation flag). A **faithful** projection — no
/// re-minting, no counter reset.
pub(crate) fn to_crate_goal(goal: &ThreadGoal) -> CrateThreadGoal {
    CrateThreadGoal {
        thread_id: goal.thread_id.clone(),
        goal_id: goal.goal_id.clone(),
        objective: goal.objective.clone(),
        status: to_crate_status(goal.status),
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
        created_at_ms: goal.created_at_ms,
        updated_at_ms: goal.updated_at_ms,
        continuation_suppressed: goal.continuation_suppressed,
    }
}

/// Convert a crate [`CrateThreadGoal`] back into a legacy [`ThreadGoal`] (the
/// inverse of [`to_crate_goal`]), used by the store adapter to return
/// local goals from the crate store.
pub(crate) fn from_crate_goal(goal: &CrateThreadGoal) -> ThreadGoal {
    ThreadGoal {
        thread_id: goal.thread_id.clone(),
        goal_id: goal.goal_id.clone(),
        objective: goal.objective.clone(),
        status: from_crate_status(goal.status),
        token_budget: goal.token_budget,
        tokens_used: goal.tokens_used,
        time_used_seconds: goal.time_used_seconds,
        created_at_ms: goal.created_at_ms,
        updated_at_ms: goal.updated_at_ms,
        continuation_suppressed: goal.continuation_suppressed,
    }
}

/// Write the faithful crate mirror of `goal` into `store` (ns `graph.goals`,
/// key `hex(thread_id)`). Overwrites any prior mirror; idempotent for an
/// unchanged value.
pub(crate) async fn put_mirror(store: &Arc<dyn Store>, goal: &ThreadGoal) -> Result<(), String> {
    let crate_goal = to_crate_goal(goal);
    let value =
        serde_json::to_value(&crate_goal).map_err(|e| format!("serialize crate goal: {e}"))?;
    store
        .put(GOALS_NAMESPACE, &goal_key(&goal.thread_id), value)
        .await
        .map_err(|e| format!("mirror thread goal into {GOALS_NAMESPACE}: {e}"))
}

/// Read the current crate mirror for `thread_id`, or `None`. Skips a mirror that
/// fails to decode (treated as absent) so a legacy/corrupt row can't wedge the
/// shadow path.
pub(crate) async fn get_mirror(
    store: &Arc<dyn Store>,
    thread_id: &str,
) -> Result<Option<ThreadGoal>, String> {
    let value = store
        .get(GOALS_NAMESPACE, &goal_key(thread_id))
        .await
        .map_err(|e| format!("read crate goal mirror: {e}"))?;
    match value {
        Some(v) => match serde_json::from_value::<CrateThreadGoal>(v) {
            Ok(crate_goal) => Ok(Some(from_crate_goal(&crate_goal))),
            Err(e) => {
                tracing::debug!(
                    thread_id = %thread_id,
                    error = %e,
                    "[thread_goals][crate-shadow] undecodable crate mirror; treating as absent"
                );
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// Delete the crate mirror for `thread_id`. No-op when absent (matches the
/// crate/legacy clear contract).
pub(crate) async fn delete_mirror(store: &Arc<dyn Store>, thread_id: &str) -> Result<(), String> {
    store
        .delete(GOALS_NAMESPACE, &goal_key(thread_id))
        .await
        .map_err(|e| format!("delete crate goal mirror: {e}"))
}

// ── One-time legacy→crate migration helper (wired to boot via a Once) ─────────

/// Outcome of a [`migrate_legacy_goals_into_crate_store`] run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GoalMigrationReport {
    /// Legacy goal rows examined.
    pub total: usize,
    /// Rows written into the crate store because no crate value existed.
    pub copied: usize,
    /// Rows already present in the crate store and left authoritative.
    pub skipped: usize,
}

/// Copy legacy thread-goal rows missing from the crate `graph.goals` store.
///
/// **Idempotent and non-destructive**: any existing crate value is
/// authoritative and skipped, even when it differs from the stale legacy file.
/// This prevents restarts from reverting status, usage, or budget progress.
///
/// Wired into boot behind a one-shot `Once` marker in
/// `core::runtime::services::start_boot_once_jobs`. Honors the single-writer
/// constraint: run it only inside the core process.
/// Read any goals left in the retired legacy `{workspace}/thread_goals/` file-
/// JSON tree. Returns an empty vec when the directory is absent (the common
/// case after the first migration). Undecodable/unreadable files are skipped —
/// a stray file can't wedge the one-time copy.
async fn read_legacy_file_goals(workspace_dir: &Path) -> Result<Vec<ThreadGoal>, String> {
    const LEGACY_DIR: &str = "thread_goals";
    const LEGACY_EXT: &str = "json";
    let dir = workspace_dir.join(LEGACY_DIR);
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(format!(
                "read legacy thread goals dir {}: {e}",
                dir.display()
            ))
        }
    };
    let mut goals = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("iterate legacy thread goals dir: {e}"))?
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(LEGACY_EXT) {
            continue;
        }
        match tokio::fs::read_to_string(&path).await {
            Ok(body) => match serde_json::from_str::<ThreadGoal>(&body) {
                Ok(goal) => goals.push(goal),
                Err(e) => {
                    tracing::debug!(path = %path.display(), error = %e, "[thread_goals][crate-migrate] skip parse error");
                }
            },
            Err(e) => {
                tracing::debug!(path = %path.display(), error = %e, "[thread_goals][crate-migrate] skip read error");
            }
        }
    }
    Ok(goals)
}

pub async fn migrate_legacy_goals_into_crate_store(
    workspace_dir: &Path,
) -> Result<GoalMigrationReport, String> {
    let legacy = read_legacy_file_goals(workspace_dir).await?;
    let store = crate_goals_store(workspace_dir);
    let mut report = GoalMigrationReport {
        total: legacy.len(),
        ..Default::default()
    };
    tracing::info!(
        workspace = %workspace_dir.display(),
        total = report.total,
        "[thread_goals][crate-migrate] start copy legacy goals → graph.goals"
    );
    for goal in &legacy {
        match store.get(GOALS_NAMESPACE, &goal_key(&goal.thread_id)).await {
            Ok(Some(_)) => {
                report.skipped += 1;
                tracing::debug!(
                    thread_id = %goal.thread_id,
                    goal_id = %goal.goal_id,
                    "[thread_goals][crate-migrate] skip (crate goal already authoritative)"
                );
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                return Err(format!(
                    "check crate goal before migrating thread {}: {e}",
                    goal.thread_id
                ))
            }
        }
        put_mirror(&store, goal).await?;
        report.copied += 1;
        tracing::debug!(
            thread_id = %goal.thread_id,
            goal_id = %goal.goal_id,
            "[thread_goals][crate-migrate] copied"
        );
    }
    tracing::info!(
        total = report.total,
        copied = report.copied,
        skipped = report.skipped,
        "[thread_goals][crate-migrate] done"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_goal(status: ThreadGoalStatus) -> ThreadGoal {
        ThreadGoal {
            thread_id: "thread-α".into(),
            goal_id: "goal-uuid-1".into(),
            objective: "ship the migration".into(),
            status,
            token_budget: Some(5_000),
            tokens_used: 1_234,
            time_used_seconds: 42,
            created_at_ms: 1_000,
            updated_at_ms: 2_000,
            continuation_suppressed: true,
        }
    }

    #[test]
    fn status_mapping_is_bijective_across_all_variants() {
        for status in [
            ThreadGoalStatus::Active,
            ThreadGoalStatus::Paused,
            ThreadGoalStatus::BudgetLimited,
            ThreadGoalStatus::Complete,
        ] {
            let round = from_crate_status(to_crate_status(status));
            assert_eq!(round, status, "status round-trip must be identity");
        }
        // Pin the exact crate labels the mapping produces.
        assert_eq!(to_crate_status(ThreadGoalStatus::Active).as_str(), "active");
        assert_eq!(to_crate_status(ThreadGoalStatus::Paused).as_str(), "paused");
        assert_eq!(
            to_crate_status(ThreadGoalStatus::BudgetLimited).as_str(),
            "budget_limited"
        );
        assert_eq!(
            to_crate_status(ThreadGoalStatus::Complete).as_str(),
            "complete"
        );
    }

    #[test]
    fn goal_mapping_preserves_every_field_and_completion_contract() {
        // Completion contract: Complete + continuation_suppressed carries through.
        let g = sample_goal(ThreadGoalStatus::Complete);
        let crate_goal = to_crate_goal(&g);
        assert_eq!(crate_goal.thread_id, g.thread_id);
        assert_eq!(
            crate_goal.goal_id, g.goal_id,
            "goal_id preserved (no re-mint)"
        );
        assert_eq!(crate_goal.objective, g.objective);
        assert_eq!(crate_goal.status, CrateStatus::Complete);
        assert_eq!(crate_goal.token_budget, g.token_budget, "budget preserved");
        assert_eq!(crate_goal.tokens_used, g.tokens_used, "usage preserved");
        assert_eq!(crate_goal.time_used_seconds, g.time_used_seconds);
        assert_eq!(crate_goal.created_at_ms, g.created_at_ms);
        assert_eq!(crate_goal.updated_at_ms, g.updated_at_ms);
        assert!(
            crate_goal.continuation_suppressed,
            "completion suppresses continuation"
        );
        // Full round-trip identity.
        assert_eq!(from_crate_goal(&crate_goal), g);
    }

    #[test]
    fn budget_limited_maps_and_over_budget_carries() {
        let mut g = sample_goal(ThreadGoalStatus::BudgetLimited);
        g.tokens_used = 6_000; // over the 5_000 budget
        let crate_goal = to_crate_goal(&g);
        assert_eq!(crate_goal.status, CrateStatus::BudgetLimited);
        assert!(crate_goal.over_budget(), "over-budget invariant carries");
        assert_eq!(crate_goal.budget_remaining(), Some(0));
    }

    #[tokio::test]
    async fn put_get_delete_mirror_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate_goals_store(tmp.path());
        let g = sample_goal(ThreadGoalStatus::Active);

        assert!(get_mirror(&store, &g.thread_id).await.unwrap().is_none());
        put_mirror(&store, &g).await.unwrap();
        let read = get_mirror(&store, &g.thread_id).await.unwrap().unwrap();
        assert_eq!(read, g, "mirror round-trips the exact legacy value");

        delete_mirror(&store, &g.thread_id).await.unwrap();
        assert!(get_mirror(&store, &g.thread_id).await.unwrap().is_none());
        // Delete is idempotent (no-op when absent).
        delete_mirror(&store, &g.thread_id).await.unwrap();
    }

    #[tokio::test]
    async fn crate_reader_resolves_the_mirrored_key() {
        // Proves the key/namespace we write matches what the crate's own
        // `graph::goals::store` reader computes — the whole point of the mirror.
        let tmp = tempfile::tempdir().unwrap();
        let store = crate_goals_store(tmp.path());
        let g = sample_goal(ThreadGoalStatus::Paused);
        put_mirror(&store, &g).await.unwrap();

        let via_crate = tinyagents::graph::goals::store::get(&store, &g.thread_id)
            .await
            .unwrap()
            .expect("crate reader finds the mirrored row");
        assert_eq!(via_crate.goal_id, g.goal_id);
        assert_eq!(via_crate.status, CrateStatus::Paused);
        assert_eq!(via_crate.tokens_used, g.tokens_used);
    }

    /// Write a goal into the retired legacy `{workspace}/thread_goals/` file-JSON
    /// tree (the shape `read_legacy_file_goals` reads), so the migration has
    /// something to copy.
    fn write_legacy_goal(dir: &std::path::Path, goal: &ThreadGoal) {
        let legacy_dir = dir.join("thread_goals");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let path = legacy_dir.join(format!("{}.json", goal.thread_id));
        std::fs::write(&path, serde_json::to_string(goal).unwrap()).unwrap();
    }

    fn legacy_goal(thread_id: &str, objective: &str, tokens_used: u64) -> ThreadGoal {
        ThreadGoal {
            thread_id: thread_id.into(),
            goal_id: format!("goal-{thread_id}"),
            objective: objective.into(),
            status: ThreadGoalStatus::Active,
            token_budget: None,
            tokens_used,
            time_used_seconds: 0,
            created_at_ms: 1_000,
            updated_at_ms: 2_000,
            continuation_suppressed: false,
        }
    }

    #[tokio::test]
    async fn migration_copies_then_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // Seed two goals into the legacy file-JSON tree.
        write_legacy_goal(dir, &legacy_goal("t1", "objective one", 0));
        write_legacy_goal(dir, &legacy_goal("t2", "objective two", 50));

        // First run copies both.
        let r1 = migrate_legacy_goals_into_crate_store(dir).await.unwrap();
        assert_eq!(r1.total, 2);
        assert_eq!(r1.copied, 2);
        assert_eq!(r1.skipped, 0);

        // Crate rows now match legacy rows exactly.
        let store = crate_goals_store(dir);
        let m2 = get_mirror(&store, "t2").await.unwrap().unwrap();
        assert_eq!(m2.tokens_used, 50);
        assert_eq!(m2.objective, "objective two");

        // Simulate live usage/status updates while the legacy files remain.
        let mut advanced = m2;
        advanced.tokens_used = 999;
        advanced.status = ThreadGoalStatus::Complete;
        advanced.updated_at_ms = 3_000;
        put_mirror(&store, &advanced).await.unwrap();

        // Second run is a no-op and must preserve the newer crate state.
        let r2 = migrate_legacy_goals_into_crate_store(dir).await.unwrap();
        assert_eq!(r2.total, 2);
        assert_eq!(r2.copied, 0, "idempotent: nothing re-copied");
        assert_eq!(r2.skipped, 2);
        let preserved = get_mirror(&store, "t2").await.unwrap().unwrap();
        assert_eq!(preserved.tokens_used, 999);
        assert_eq!(preserved.status, ThreadGoalStatus::Complete);
        assert_eq!(preserved.updated_at_ms, 3_000);
    }
}
