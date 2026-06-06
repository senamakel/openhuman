//! Subconscious scratchpad — persistent working memory across ticks.
//!
//! The scratchpad holds up to `MAX_ENTRIES` (default 100) short thoughts
//! that carry over between subconscious ticks, giving the agent a
//! consistent stream-of-consciousness. Entries are stored in SQLite
//! alongside the rest of the subconscious state.
//!
//! The agent manages its own scratchpad via three tools:
//! `scratchpad_add`, `scratchpad_edit`, `scratchpad_remove`.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_ENTRIES: usize = 100;

/// One scratchpad entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadEntry {
    pub id: String,
    pub body: String,
    pub priority: u32,
    pub created_at: f64,
    pub updated_at: f64,
}

pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS subconscious_scratchpad (
            id         TEXT PRIMARY KEY,
            body       TEXT NOT NULL,
            priority   INTEGER NOT NULL DEFAULT 0,
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_scratchpad_priority
            ON subconscious_scratchpad(priority DESC, updated_at DESC);",
    )
    .context("scratchpad DDL")?;
    Ok(())
}

pub fn list(conn: &Connection, limit: usize) -> Result<Vec<ScratchpadEntry>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, body, priority, created_at, updated_at
         FROM subconscious_scratchpad
         ORDER BY priority DESC, updated_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![limit as i64], |row| {
            Ok(ScratchpadEntry {
                id: row.get(0)?,
                body: row.get(1)?,
                priority: row.get::<_, i64>(2)? as u32,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn add(conn: &Connection, body: &str, priority: u32, max_entries: usize) -> Result<String> {
    ensure_table(conn)?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_secs();

    conn.execute(
        "INSERT INTO subconscious_scratchpad (id, body, priority, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, body, priority as i64, now, now],
    )?;

    // Evict oldest low-priority entries beyond the cap.
    let count: i64 =
        conn.query_row("SELECT COUNT(*) FROM subconscious_scratchpad", [], |r| {
            r.get(0)
        })?;
    if count > max_entries as i64 {
        let excess = count - max_entries as i64;
        conn.execute(
            "DELETE FROM subconscious_scratchpad WHERE id IN (
                SELECT id FROM subconscious_scratchpad
                ORDER BY priority ASC, updated_at ASC
                LIMIT ?1
            )",
            rusqlite::params![excess],
        )?;
    }

    Ok(id)
}

pub fn edit(conn: &Connection, id: &str, body: &str, priority: Option<u32>) -> Result<bool> {
    ensure_table(conn)?;
    let now = now_secs();
    let changed = if let Some(p) = priority {
        conn.execute(
            "UPDATE subconscious_scratchpad SET body = ?1, priority = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![body, p as i64, now, id],
        )?
    } else {
        conn.execute(
            "UPDATE subconscious_scratchpad SET body = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![body, now, id],
        )?
    };
    Ok(changed > 0)
}

pub fn remove(conn: &Connection, id: &str) -> Result<bool> {
    ensure_table(conn)?;
    let changed = conn.execute(
        "DELETE FROM subconscious_scratchpad WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(changed > 0)
}

pub fn render_for_prompt(entries: &[ScratchpadEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Scratchpad (your persistent working memory)\n\n");
    out.push_str("These are your own notes from previous ticks. Update, remove, or add entries as your understanding evolves.\n\n");
    for entry in entries {
        let ts = chrono::DateTime::from_timestamp(entry.updated_at as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        out.push_str(&format!(
            "- **[{}]** (p{}) {}\n  _updated: {}_\n",
            entry.id, entry.priority, entry.body, ts
        ));
    }
    out
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn add_and_list_round_trip() {
        let conn = mem_conn();
        let id = add(&conn, "first thought", 1, 100).unwrap();
        let entries = list(&conn, 100).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].body, "first thought");
        assert_eq!(entries[0].priority, 1);
    }

    #[test]
    fn edit_updates_body_and_timestamp() {
        let conn = mem_conn();
        let id = add(&conn, "old", 0, 100).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let found = edit(&conn, &id, "new", Some(5)).unwrap();
        assert!(found);
        let entries = list(&conn, 100).unwrap();
        assert_eq!(entries[0].body, "new");
        assert_eq!(entries[0].priority, 5);
    }

    #[test]
    fn remove_deletes_entry() {
        let conn = mem_conn();
        let id = add(&conn, "gone", 0, 100).unwrap();
        assert!(remove(&conn, &id).unwrap());
        assert!(list(&conn, 100).unwrap().is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let conn = mem_conn();
        assert!(!remove(&conn, "nope").unwrap());
    }

    #[test]
    fn add_evicts_oldest_low_priority_beyond_cap() {
        let conn = mem_conn();
        for i in 0..5 {
            add(&conn, &format!("note {i}"), 0, 100).unwrap();
        }
        add(&conn, "high priority", 10, 100).unwrap();
        // Now cap at 3 — should evict the 3 oldest low-priority
        add(&conn, "newest", 0, 3).unwrap();
        let entries = list(&conn, 100).unwrap();
        assert!(entries.len() <= 3);
        assert!(entries.iter().any(|e| e.body == "high priority"));
    }

    #[test]
    fn render_for_prompt_formats_entries() {
        let entries = vec![ScratchpadEntry {
            id: "abc".to_string(),
            body: "test thought".to_string(),
            priority: 2,
            created_at: 1700000000.0,
            updated_at: 1700000000.0,
        }];
        let rendered = render_for_prompt(&entries);
        assert!(rendered.contains("## Scratchpad"));
        assert!(rendered.contains("[abc]"));
        assert!(rendered.contains("test thought"));
        assert!(rendered.contains("(p2)"));
    }

    #[test]
    fn render_for_prompt_empty_returns_empty() {
        assert!(render_for_prompt(&[]).is_empty());
    }
}
