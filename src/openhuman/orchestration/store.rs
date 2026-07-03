//! SQLite persistence for the orchestration domain.
//!
//! Lives at `<workspace>/orchestration/orchestration.db`. Message bodies are
//! decrypted plaintext, so this path is workspace-internal (protected by
//! `is_workspace_internal_path`). Follows the subconscious/cron `with_connection`
//! pattern.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use super::types::{OrchestrationMessage, OrchestrationSession};

const SCHEMA_DDL: &str = "
    PRAGMA foreign_keys = ON;

    CREATE TABLE IF NOT EXISTS sessions (
        session_id      TEXT NOT NULL,
        agent_id        TEXT NOT NULL,
        source          TEXT NOT NULL,
        label           TEXT,
        workspace       TEXT,
        last_seq        INTEGER NOT NULL DEFAULT 0,
        created_at      TEXT NOT NULL,
        last_message_at TEXT NOT NULL,
        PRIMARY KEY (agent_id, session_id)
    );

    CREATE TABLE IF NOT EXISTS messages (
        id         TEXT PRIMARY KEY,
        agent_id   TEXT NOT NULL,
        session_id TEXT NOT NULL,
        chat_kind  TEXT NOT NULL,
        role       TEXT NOT NULL,
        body       TEXT NOT NULL,
        timestamp  TEXT NOT NULL,
        seq        INTEGER NOT NULL DEFAULT 0
    );

    CREATE INDEX IF NOT EXISTS idx_messages_session
        ON messages (agent_id, session_id, timestamp);

    CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT NOT NULL);
";

/// Open the orchestration DB, initialise the schema, and run `f`.
pub fn with_connection<T>(
    workspace_dir: &Path,
    f: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    let db_path = workspace_dir.join("orchestration").join("orchestration.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create orchestration dir: {}", parent.display()))?;
    }
    let conn = Connection::open(&db_path)
        .with_context(|| format!("open orchestration DB: {}", db_path.display()))?;
    conn.execute_batch(SCHEMA_DDL)
        .context("initialise orchestration schema")?;
    f(&conn)
}

/// True if a relay message id is already persisted. This guard MUST run before
/// decryption so the non-idempotent Signal double-ratchet is never advanced
/// twice for the same message.
pub fn message_exists(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM messages WHERE id = ?1", params![id], |_| {
            Ok(())
        })
        .optional()?
        .is_some())
}

/// Insert or update the session row (keyed by agent + session).
pub fn upsert_session(conn: &Connection, s: &OrchestrationSession) -> Result<()> {
    conn.execute(
        "INSERT INTO sessions
           (session_id, agent_id, source, label, workspace, last_seq, created_at, last_message_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(agent_id, session_id) DO UPDATE SET
           last_seq = MAX(sessions.last_seq, excluded.last_seq),
           last_message_at = excluded.last_message_at,
           label = COALESCE(excluded.label, sessions.label),
           workspace = COALESCE(excluded.workspace, sessions.workspace)",
        params![
            s.session_id,
            s.agent_id,
            s.source,
            s.label,
            s.workspace,
            s.last_seq,
            s.created_at,
            s.last_message_at,
        ],
    )?;
    Ok(())
}

/// Insert a message, idempotent by relay id. Returns true if a new row landed.
pub fn insert_message(conn: &Connection, m: &OrchestrationMessage) -> Result<bool> {
    let changed = conn.execute(
        "INSERT OR IGNORE INTO messages
           (id, agent_id, session_id, chat_kind, role, body, timestamp, seq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            m.id,
            m.agent_id,
            m.session_id,
            m.chat_kind.as_str(),
            m.role,
            m.body,
            m.timestamp,
            m.seq,
        ],
    )?;
    Ok(changed > 0)
}

/// Count persisted messages for a session (test/observability helper).
pub fn count_messages(conn: &Connection, agent_id: &str, session_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE agent_id = ?1 AND session_id = ?2",
        params![agent_id, session_id],
        |row| row.get(0),
    )?)
}

/// Load a single session row (the wake graph's counterpart + metadata).
pub fn load_session(
    conn: &Connection,
    agent_id: &str,
    session_id: &str,
) -> Result<Option<OrchestrationSession>> {
    conn.query_row(
        "SELECT session_id, agent_id, source, label, workspace, last_seq, created_at, last_message_at
           FROM sessions WHERE agent_id = ?1 AND session_id = ?2",
        params![agent_id, session_id],
        |row| {
            Ok(OrchestrationSession {
                session_id: row.get(0)?,
                agent_id: row.get(1)?,
                source: row.get(2)?,
                label: row.get(3)?,
                workspace: row.get(4)?,
                last_seq: row.get(5)?,
                created_at: row.get(6)?,
                last_message_at: row.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Load the most recent `limit` messages for a session, returned in chronological
/// (oldest-first) order so the graph reads them like a transcript.
pub fn list_recent_messages(
    conn: &Connection,
    agent_id: &str,
    session_id: &str,
    limit: u32,
) -> Result<Vec<OrchestrationMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_id, session_id, chat_kind, role, body, timestamp, seq
           FROM messages WHERE agent_id = ?1 AND session_id = ?2
           ORDER BY timestamp DESC, seq DESC LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![agent_id, session_id, limit], |row| {
            let chat_kind: String = row.get(3)?;
            Ok(OrchestrationMessage {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                session_id: row.get(2)?,
                chat_kind: crate::openhuman::orchestration::types::ChatKind::from_str(&chat_kind),
                role: row.get(4)?,
                body: row.get(5)?,
                timestamp: row.get(6)?,
                seq: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // Reverse the DESC scan back to chronological order.
    Ok(rows.into_iter().rev().collect())
}

/// Read a `kv` value (used for the per-session idempotence cursor).
pub fn kv_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT v FROM kv WHERE k = ?1", params![key], |r| r.get(0))
        .optional()
        .map_err(Into::into)
}

/// Write a `kv` value (upsert).
pub fn kv_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO kv (k, v) VALUES (?1, ?2)
           ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::types::ChatKind;
    use super::*;

    fn msg(id: &str, agent: &str, session: &str, seq: i64) -> OrchestrationMessage {
        OrchestrationMessage {
            id: id.into(),
            agent_id: agent.into(),
            session_id: session.into(),
            chat_kind: ChatKind::Session,
            role: "agent".into(),
            body: "hi".into(),
            timestamp: "2026-07-02T00:00:00Z".into(),
            seq,
        }
    }

    fn session(agent: &str, session: &str, seq: i64) -> OrchestrationSession {
        OrchestrationSession {
            session_id: session.into(),
            agent_id: agent.into(),
            source: "claude".into(),
            label: None,
            workspace: None,
            last_seq: seq,
            created_at: "2026-07-02T00:00:00Z".into(),
            last_message_at: "2026-07-02T00:00:00Z".into(),
        }
    }

    #[test]
    fn persists_and_dedupes_by_message_id() {
        let tmp = tempfile::tempdir().unwrap();
        with_connection(tmp.path(), |conn| {
            upsert_session(conn, &session("@a", "h1", 1))?;
            assert!(!message_exists(conn, "m1")?);
            assert!(insert_message(conn, &msg("m1", "@a", "h1", 1))?);
            // Replay of the same id is a no-op and stays deduped.
            assert!(!insert_message(conn, &msg("m1", "@a", "h1", 1))?);
            assert!(message_exists(conn, "m1")?);
            assert_eq!(count_messages(conn, "@a", "h1")?, 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn upsert_advances_last_seq_monotonically() {
        let tmp = tempfile::tempdir().unwrap();
        with_connection(tmp.path(), |conn| {
            upsert_session(conn, &session("@a", "h1", 5))?;
            upsert_session(conn, &session("@a", "h1", 2))?; // lower seq must not regress
            let seq: i64 = conn.query_row(
                "SELECT last_seq FROM sessions WHERE agent_id='@a' AND session_id='h1'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(seq, 5);
            Ok(())
        })
        .unwrap();
    }
}
