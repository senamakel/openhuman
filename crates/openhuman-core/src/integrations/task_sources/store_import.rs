//! One-shot import of the legacy `sources.db` tables into the document layout
//! (`store_documents.rs`): `task_sources` becomes one `task_sources` document
//! per source (the store's own `to_doc`) and `ingested_tasks` one
//! `ingested_tasks` document per dedup-ledger entry, with `ingested_ms`
//! derived from `ingested_at`. A source row that no longer decodes fails the
//! import, leaving the legacy tables in service. The driver
//! ([`crate::storage::local`]) writes the documents and then renames the
//! tables to `_legacy_<name>`.

use anyhow::{Context, Result};
use chrono::DateTime;
use rusqlite::Connection;
use serde_json::{json, Map, Value};

use super::super::store_documents::{ingested_id, to_doc, INGESTED, SOURCES};
use crate::config::Config;
use crate::storage::local::{table_exists, ImportDoc};

/// The legacy tables this import retires.
pub(crate) const TABLES: &[&str] = &["task_sources", "ingested_tasks"];

/// Reads every legacy row, after bringing an old schema forward.
pub(crate) fn read(config: &Config) -> Result<Vec<ImportDoc>> {
    super::with_connection(config, read_rows)
}

fn read_rows(conn: &Connection) -> Result<Vec<ImportDoc>> {
    let mut docs = Vec::new();
    if table_exists(conn, "task_sources")? {
        let mut stmt = conn
            .prepare(&format!(
                "{} ORDER BY created_at ASC, id ASC",
                super::SELECT_SOURCE_COLUMNS
            ))
            .context("[task_sources::import] prepare task_sources")?;
        let rows = stmt
            .query_map([], super::map_source_row)
            .context("[task_sources::import] query task_sources")?;
        for row in rows {
            // A row that does not decode fails the import, so the legacy
            // table is not retired with a source missing from the new one.
            let source = row.context("[task_sources::import] decode a task_sources row")?;
            docs.push(ImportDoc {
                collection: SOURCES,
                id: source.id.clone(),
                doc: to_doc(&source)?,
            });
        }
    }
    if table_exists(conn, "ingested_tasks")? {
        let mut stmt = conn
            .prepare(
                "SELECT source_id, external_id, content_hash, title, payload, ingested_at
                 FROM ingested_tasks ORDER BY rowid",
            )
            .context("[task_sources::import] prepare ingested_tasks")?;
        let rows = stmt
            .query_map([], |row| {
                let source_id: String = row.get(0)?;
                let external_id: String = row.get(1)?;
                let ingested_at: String = row.get(5)?;
                let mut doc = Map::new();
                doc.insert("source_id".into(), json!(source_id));
                doc.insert("external_id".into(), json!(external_id));
                doc.insert("content_hash".into(), json!(row.get::<_, String>(2)?));
                if let Some(title) = row.get::<_, Option<String>>(3)? {
                    doc.insert("title".into(), json!(title));
                }
                if let Some(payload) = row.get::<_, Option<String>>(4)? {
                    doc.insert("payload".into(), json!(payload));
                }
                let ingested = DateTime::parse_from_rfc3339(&ingested_at).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                doc.insert("ingested_ms".into(), json!(ingested.timestamp_millis()));
                doc.insert("ingested_at".into(), json!(ingested_at));
                Ok(ImportDoc {
                    collection: INGESTED,
                    id: ingested_id(&source_id, &external_id),
                    doc: Value::Object(doc),
                })
            })
            .context("[task_sources::import] query ingested_tasks")?;
        for row in rows {
            docs.push(row.context("[task_sources::import] decode an ingested_tasks row")?);
        }
    }
    Ok(docs)
}

#[cfg(all(test, feature = "storage-sqlite"))]
#[path = "store_import_tests.rs"]
mod tests;
