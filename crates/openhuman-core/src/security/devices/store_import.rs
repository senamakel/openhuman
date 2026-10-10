//! One-shot import of the legacy `devices.db` `paired_devices` table into the
//! document layout (`store_documents.rs`): one `paired_devices` document per
//! `channel_id`, with the columns as fields and `revoked` as a boolean.
//! `shared_secret_encrypted` was never written by the store and is not
//! carried over. The driver ([`crate::storage::local`]) writes the documents
//! and then renames the table to `_legacy_paired_devices`.

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

use super::super::store_documents::DEVICES;
use crate::config::Config;
use crate::storage::local::{table_exists, ImportDoc};

/// The legacy tables this import retires.
pub(crate) const TABLES: &[&str] = &["paired_devices"];

/// Reads every legacy row.
pub(crate) fn read(config: &Config) -> Result<Vec<ImportDoc>> {
    super::with_connection(config, read_rows)
}

fn read_rows(conn: &Connection) -> Result<Vec<ImportDoc>> {
    if !table_exists(conn, "paired_devices")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare(
            "SELECT channel_id, label, device_pubkey, core_session_token_hash,
                    created_at, last_seen_at, revoked
             FROM paired_devices ORDER BY rowid",
        )
        .context("[devices::import] prepare paired_devices")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ImportDoc {
                collection: DEVICES,
                id: row.get(0)?,
                doc: json!({
                    "label": row.get::<_, String>(1)?,
                    "device_pubkey": row.get::<_, String>(2)?,
                    "core_session_token_hash": row.get::<_, String>(3)?,
                    "created_at": row.get::<_, String>(4)?,
                    "last_seen_at": row
                        .get::<_, Option<String>>(5)?
                        .map_or(Value::Null, Value::String),
                    "revoked": row.get::<_, i64>(6)? != 0,
                }),
            })
        })
        .context("[devices::import] query paired_devices")?;
    rows.map(|row| row.context("[devices::import] decode a paired_devices row"))
        .collect()
}

#[cfg(all(test, feature = "storage-sqlite"))]
#[path = "store_import_tests.rs"]
mod tests;
