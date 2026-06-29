//! The [`Checkpointer`] trait + an in-memory implementation for tests.
//!
//! Modelled on LangGraph's checkpointer abstraction: a pluggable backend that
//! persists run state so a graph can pause (human-in-the-loop) and resume later
//! without losing progress. The production backend is
//! [`SqliteCheckpointer`](super::store::SqliteCheckpointer).

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;

use super::types::{Checkpoint, GraphRunRecord};

/// Storage-agnostic persistence for graph runs + checkpoints.
///
/// All state is type-erased as JSON (see [`GraphRunRecord::state`]). Callers
/// serialize their concrete [`GraphState`](crate::openhuman::agent_graph::graph::GraphState)
/// before saving and deserialize after loading.
#[async_trait]
pub trait Checkpointer: Send + Sync {
    /// Insert or update a run record (upsert keyed by `run_id`).
    async fn save_run(&self, rec: &GraphRunRecord) -> Result<()>;
    /// Load a run by id.
    async fn load_run(&self, run_id: &str) -> Result<Option<GraphRunRecord>>;
    /// List runs newest-first, paged.
    async fn list_runs(&self, limit: usize, offset: usize) -> Result<Vec<GraphRunRecord>>;
    /// Append a checkpoint snapshot. Returns the assigned id.
    async fn save_checkpoint(&self, cp: &Checkpoint) -> Result<i64>;
    /// List a run's checkpoints, oldest-first.
    async fn list_checkpoints(&self, run_id: &str) -> Result<Vec<Checkpoint>>;
    /// The most recent checkpoint for a run, if any.
    async fn latest_checkpoint(&self, run_id: &str) -> Result<Option<Checkpoint>> {
        Ok(self.list_checkpoints(run_id).await?.into_iter().next_back())
    }
}

/// In-memory checkpointer for unit tests and ephemeral runs. Not durable.
#[derive(Default)]
pub struct InMemoryCheckpointer {
    runs: Mutex<HashMap<String, GraphRunRecord>>,
    checkpoints: Mutex<Vec<Checkpoint>>,
    next_id: Mutex<i64>,
}

impl InMemoryCheckpointer {
    /// Fresh empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Checkpointer for InMemoryCheckpointer {
    async fn save_run(&self, rec: &GraphRunRecord) -> Result<()> {
        self.runs
            .lock()
            .unwrap()
            .insert(rec.run_id.clone(), rec.clone());
        Ok(())
    }

    async fn load_run(&self, run_id: &str) -> Result<Option<GraphRunRecord>> {
        Ok(self.runs.lock().unwrap().get(run_id).cloned())
    }

    async fn list_runs(&self, limit: usize, offset: usize) -> Result<Vec<GraphRunRecord>> {
        let mut runs: Vec<GraphRunRecord> = self.runs.lock().unwrap().values().cloned().collect();
        // Newest-first by creation time (RFC-3339 sorts lexicographically).
        runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(runs.into_iter().skip(offset).take(limit).collect())
    }

    async fn save_checkpoint(&self, cp: &Checkpoint) -> Result<i64> {
        let mut id_guard = self.next_id.lock().unwrap();
        *id_guard += 1;
        let id = *id_guard;
        let mut stored = cp.clone();
        stored.id = id;
        self.checkpoints.lock().unwrap().push(stored);
        Ok(id)
    }

    async fn list_checkpoints(&self, run_id: &str) -> Result<Vec<Checkpoint>> {
        let mut cps: Vec<Checkpoint> = self
            .checkpoints
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.run_id == run_id)
            .cloned()
            .collect();
        cps.sort_by_key(|c| c.id);
        Ok(cps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::agent_graph::types::GraphRunStatus;
    use serde_json::json;

    fn run(id: &str, created: &str) -> GraphRunRecord {
        GraphRunRecord {
            run_id: id.to_string(),
            graph_name: "demo".to_string(),
            status: GraphRunStatus::Running,
            created_at: created.to_string(),
            updated_at: created.to_string(),
            last_node: "a".to_string(),
            steps: 1,
            state: json!({"k": 1}),
            interrupt: None,
            error: None,
            transitions: vec![],
        }
    }

    #[tokio::test]
    async fn upsert_and_load_run() {
        let cp = InMemoryCheckpointer::new();
        cp.save_run(&run("r1", "2026-01-01T00:00:00Z"))
            .await
            .unwrap();
        let mut updated = run("r1", "2026-01-01T00:00:00Z");
        updated.steps = 5;
        cp.save_run(&updated).await.unwrap();
        let loaded = cp.load_run("r1").await.unwrap().unwrap();
        assert_eq!(loaded.steps, 5);
        assert!(cp.load_run("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_runs_newest_first_paged() {
        let cp = InMemoryCheckpointer::new();
        cp.save_run(&run("old", "2026-01-01T00:00:00Z"))
            .await
            .unwrap();
        cp.save_run(&run("new", "2026-02-01T00:00:00Z"))
            .await
            .unwrap();
        let runs = cp.list_runs(10, 0).await.unwrap();
        assert_eq!(runs[0].run_id, "new");
        assert_eq!(runs[1].run_id, "old");
        let paged = cp.list_runs(1, 1).await.unwrap();
        assert_eq!(paged.len(), 1);
        assert_eq!(paged[0].run_id, "old");
    }

    #[tokio::test]
    async fn checkpoints_ordered_and_latest() {
        let cp = InMemoryCheckpointer::new();
        for step in 0..3u32 {
            cp.save_checkpoint(&Checkpoint {
                id: 0,
                run_id: "r1".to_string(),
                step,
                node: format!("n{step}"),
                label: "snap".to_string(),
                state: json!({"step": step}),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .await
            .unwrap();
        }
        let list = cp.list_checkpoints("r1").await.unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].step, 0);
        let latest = cp.latest_checkpoint("r1").await.unwrap().unwrap();
        assert_eq!(latest.step, 2);
    }
}
