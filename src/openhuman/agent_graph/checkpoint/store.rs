//! SQLite-backed [`Checkpointer`] — the durable production backend.
//!
//! Mirrors the `cron` store's `with_connection` pattern (open-on-demand,
//! `CREATE TABLE IF NOT EXISTS`). DB lives at
//! `{workspace}/.openhuman/agent_graph/checkpoints.db`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection};

use super::checkpointer::Checkpointer;
use super::types::{Checkpoint, GraphRunRecord};
use crate::openhuman::agent_graph::types::{GraphRunStatus, InterruptRequest, NodeTransition};

/// Durable, queryable checkpointer over SQLite.
pub struct SqliteCheckpointer {
    workspace_dir: PathBuf,
}

impl SqliteCheckpointer {
    /// Construct against a workspace directory (typically `config.workspace_dir`).
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self { workspace_dir }
    }

    fn db_path(&self) -> PathBuf {
        self.workspace_dir
            .join(".openhuman")
            .join("agent_graph")
            .join("checkpoints.db")
    }

    /// Open the DB (creating the dir + schema on first use) and run `f`.
    fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let db_path = self.db_path();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "[agent_graph] create checkpoint dir failed: {}",
                    parent.display()
                )
            })?;
        }
        let conn = Connection::open(&db_path).with_context(|| {
            format!(
                "[agent_graph] open checkpoint DB failed: {}",
                db_path.display()
            )
        })?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS graph_runs (
                run_id      TEXT PRIMARY KEY,
                graph_name  TEXT NOT NULL,
                status      TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                last_node   TEXT NOT NULL,
                steps       INTEGER NOT NULL DEFAULT 0,
                state       TEXT NOT NULL,
                interrupt   TEXT,
                error       TEXT,
                transitions TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_graph_runs_created ON graph_runs(created_at);
             CREATE INDEX IF NOT EXISTS idx_graph_runs_status ON graph_runs(status);

             CREATE TABLE IF NOT EXISTS graph_checkpoints (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id      TEXT NOT NULL,
                step        INTEGER NOT NULL,
                node        TEXT NOT NULL,
                label       TEXT NOT NULL,
                state       TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES graph_runs(run_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS idx_graph_checkpoints_run ON graph_checkpoints(run_id, id);",
        )
        .context("[agent_graph] init checkpoint schema failed")?;
        f(&conn)
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphRunRecord> {
    let status: String = row.get("status")?;
    let state_str: String = row.get("state")?;
    let interrupt_str: Option<String> = row.get("interrupt")?;
    let transitions_str: Option<String> = row.get("transitions")?;
    Ok(GraphRunRecord {
        run_id: row.get("run_id")?,
        graph_name: row.get("graph_name")?,
        status: GraphRunStatus::from_str(&status),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_node: row.get("last_node")?,
        steps: row.get::<_, i64>("steps")? as u32,
        state: serde_json::from_str(&state_str).unwrap_or(serde_json::Value::Null),
        interrupt: interrupt_str.and_then(|s| serde_json::from_str::<InterruptRequest>(&s).ok()),
        error: row.get("error")?,
        transitions: transitions_str
            .and_then(|s| serde_json::from_str::<Vec<NodeTransition>>(&s).ok())
            .unwrap_or_default(),
    })
}

#[async_trait]
impl Checkpointer for SqliteCheckpointer {
    async fn save_run(&self, rec: &GraphRunRecord) -> Result<()> {
        let state = serde_json::to_string(&rec.state)?;
        let interrupt = rec
            .interrupt
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let transitions = serde_json::to_string(&rec.transitions)?;
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO graph_runs
                    (run_id, graph_name, status, created_at, updated_at, last_node, steps, state, interrupt, error, transitions)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(run_id) DO UPDATE SET
                    status=excluded.status,
                    updated_at=excluded.updated_at,
                    last_node=excluded.last_node,
                    steps=excluded.steps,
                    state=excluded.state,
                    interrupt=excluded.interrupt,
                    error=excluded.error,
                    transitions=excluded.transitions",
                params![
                    rec.run_id,
                    rec.graph_name,
                    rec.status.as_str(),
                    rec.created_at,
                    rec.updated_at,
                    rec.last_node,
                    rec.steps as i64,
                    state,
                    interrupt,
                    rec.error,
                    transitions,
                ],
            )
            .context("[agent_graph] save_run failed")?;
            Ok(())
        })
    }

    async fn load_run(&self, run_id: &str) -> Result<Option<GraphRunRecord>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM graph_runs WHERE run_id = ?1")?;
            let mut rows = stmt.query_map([run_id], row_to_run)?;
            match rows.next() {
                Some(r) => Ok(Some(r?)),
                None => Ok(None),
            }
        })
    }

    async fn list_runs(&self, limit: usize, offset: usize) -> Result<Vec<GraphRunRecord>> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM graph_runs ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")?;
            let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_run)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    async fn save_checkpoint(&self, cp: &Checkpoint) -> Result<i64> {
        let state = serde_json::to_string(&cp.state)?;
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO graph_checkpoints (run_id, step, node, label, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    cp.run_id,
                    cp.step as i64,
                    cp.node,
                    cp.label,
                    state,
                    cp.created_at
                ],
            )
            .context("[agent_graph] save_checkpoint failed")?;
            Ok(conn.last_insert_rowid())
        })
    }

    async fn list_checkpoints(&self, run_id: &str) -> Result<Vec<Checkpoint>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, run_id, step, node, label, state, created_at
                 FROM graph_checkpoints WHERE run_id = ?1 ORDER BY id ASC",
            )?;
            let rows = stmt.query_map([run_id], |row| {
                let state_str: String = row.get("state")?;
                Ok(Checkpoint {
                    id: row.get("id")?,
                    run_id: row.get("run_id")?,
                    step: row.get::<_, i64>("step")? as u32,
                    node: row.get("node")?,
                    label: row.get("label")?,
                    state: serde_json::from_str(&state_str).unwrap_or(serde_json::Value::Null),
                    created_at: row.get("created_at")?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, SqliteCheckpointer) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteCheckpointer::new(dir.path().to_path_buf());
        (dir, store)
    }

    fn rec(id: &str) -> GraphRunRecord {
        GraphRunRecord {
            run_id: id.to_string(),
            graph_name: "demo".to_string(),
            status: GraphRunStatus::Paused,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            last_node: "hitl".to_string(),
            steps: 2,
            state: json!({"answer": 42}),
            interrupt: Some(InterruptRequest {
                kind: "approval".to_string(),
                question: "ok?".to_string(),
                options: vec!["yes".to_string()],
                resume_to: Some("after".to_string()),
            }),
            error: None,
            transitions: vec![NodeTransition {
                step: 0,
                node: "hitl".to_string(),
                command: "interrupt:approval".to_string(),
                elapsed_ms: 3,
                ts: "2026-01-01T00:00:00Z".to_string(),
            }],
        }
    }

    #[tokio::test]
    async fn run_roundtrip_preserves_interrupt_and_transitions() {
        let (_dir, store) = store();
        store.save_run(&rec("r1")).await.unwrap();
        let loaded = store.load_run("r1").await.unwrap().unwrap();
        assert_eq!(loaded.status, GraphRunStatus::Paused);
        assert_eq!(loaded.state, json!({"answer": 42}));
        assert_eq!(
            loaded.interrupt.unwrap().resume_to.as_deref(),
            Some("after")
        );
        assert_eq!(loaded.transitions.len(), 1);
    }

    #[tokio::test]
    async fn save_run_upserts() {
        let (_dir, store) = store();
        store.save_run(&rec("r1")).await.unwrap();
        let mut updated = rec("r1");
        updated.status = GraphRunStatus::Completed;
        updated.steps = 9;
        store.save_run(&updated).await.unwrap();
        let loaded = store.load_run("r1").await.unwrap().unwrap();
        assert_eq!(loaded.status, GraphRunStatus::Completed);
        assert_eq!(loaded.steps, 9);
        // Still a single row.
        assert_eq!(store.list_runs(10, 0).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn checkpoints_persist_and_order() {
        let (_dir, store) = store();
        store.save_run(&rec("r1")).await.unwrap();
        for step in 0..3u32 {
            let id = store
                .save_checkpoint(&Checkpoint {
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
            assert!(id > 0);
        }
        let list = store.list_checkpoints("r1").await.unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].step, 0);
        assert_eq!(
            store.latest_checkpoint("r1").await.unwrap().unwrap().step,
            2
        );
    }

    #[tokio::test]
    async fn list_runs_newest_first() {
        let (_dir, store) = store();
        let mut a = rec("old");
        a.created_at = "2026-01-01T00:00:00Z".to_string();
        let mut b = rec("new");
        b.created_at = "2026-03-01T00:00:00Z".to_string();
        store.save_run(&a).await.unwrap();
        store.save_run(&b).await.unwrap();
        let runs = store.list_runs(10, 0).await.unwrap();
        assert_eq!(runs[0].run_id, "new");
    }
}
