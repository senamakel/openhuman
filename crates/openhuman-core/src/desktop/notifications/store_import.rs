//! One-shot import of the legacy `notifications.db` tables into the document
//! layout (`store_documents.rs`).
//!
//! | Legacy table | Collection | Id |
//! | --- | --- | --- |
//! | `integration_notifications` | `integration_notifications` | row id |
//! | `notification_settings` | `notification_settings` | provider |
//! | `core_notifications` | `core_notifications` | `core_id("local", id)` |
//!
//! Documents come from the store's own `to_doc` and field names. The legacy
//! `core_notifications` rows carry no workspace, so they take the fixed
//! `LOCAL_WORKSPACE` key the per-workspace default file filters on (not the
//! directory path, which changes when the workspace is moved).
//! (The content-dedup window needs no import: it lasts a minute.) The driver
//! ([`crate::storage::local`]) writes the documents and then renames the
//! tables to `_legacy_<name>`.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::json;

use super::super::store_documents::{
    core_id, to_doc, CORE, LOCAL_WORKSPACE, NOTIFICATIONS, SETTINGS,
};
use crate::config::Config;
use crate::storage::local::{table_exists, ImportDoc};

/// The legacy tables this import retires.
pub(crate) const TABLES: &[&str] = &[
    "integration_notifications",
    "notification_settings",
    "core_notifications",
];

/// Reads every legacy row.
pub(crate) fn read(config: &Config) -> Result<Vec<ImportDoc>> {
    super::with_connection(config, |conn| read_rows(conn, LOCAL_WORKSPACE))
}

fn read_rows(conn: &Connection, workspace: &str) -> Result<Vec<ImportDoc>> {
    let mut docs = Vec::new();
    if table_exists(conn, "integration_notifications")? {
        let mut stmt = conn
            .prepare(
                "SELECT id, provider, account_id, title, body, raw_payload,
                        importance_score, triage_action, triage_reason, status,
                        received_at, scored_at
                 FROM integration_notifications ORDER BY rowid",
            )
            .context("[notifications::import] prepare integration_notifications")?;
        let rows = stmt
            .query(())
            .context("[notifications::import] query integration_notifications")?;
        for notification in super::rows_to_notifications(rows)? {
            docs.push(ImportDoc {
                collection: NOTIFICATIONS,
                id: notification.id.clone(),
                doc: to_doc(&notification),
            });
        }
    }
    if table_exists(conn, "notification_settings")? {
        let mut stmt = conn
            .prepare(
                "SELECT provider, enabled, importance_threshold, route_to_orchestrator
                 FROM notification_settings ORDER BY rowid",
            )
            .context("[notifications::import] prepare notification_settings")?;
        let rows = stmt.query_map([], |row| {
            Ok(ImportDoc {
                collection: SETTINGS,
                id: row.get(0)?,
                doc: json!({
                    "enabled": row.get::<_, i64>(1)? != 0,
                    "importance_threshold": row.get::<_, f64>(2)?,
                    "route_to_orchestrator": row.get::<_, i64>(3)? != 0,
                }),
            })
        })?;
        for row in rows {
            docs.push(row.context("[notifications::import] decode a notification_settings row")?);
        }
    }
    if table_exists(conn, "core_notifications")? {
        let mut stmt = conn
            .prepare(
                "SELECT id, payload, timestamp_ms, read, created_at
                 FROM core_notifications ORDER BY rowid",
            )
            .context("[notifications::import] prepare core_notifications")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(ImportDoc {
                collection: CORE,
                id: core_id(workspace, &id),
                doc: json!({
                    "workspace": workspace,
                    "payload": row.get::<_, String>(1)?,
                    "timestamp_ms": row.get::<_, i64>(2)?,
                    "read": row.get::<_, i64>(3)? != 0,
                    "created_at": row.get::<_, String>(4)?,
                }),
            })
        })?;
        for row in rows {
            docs.push(row.context("[notifications::import] decode a core_notifications row")?);
        }
    }
    Ok(docs)
}

#[cfg(all(test, feature = "storage-sqlite"))]
#[path = "store_import_tests.rs"]
mod tests;
