//! One-shot import of the legacy `approval.db` tables into the document
//! layout (`store_documents.rs`).
//!
//! `pending_approvals` becomes one `approvals` document per request and
//! `flow_tool_trust` one `approval_flow_trust` document per grant. Documents
//! are shaped exactly as `store_documents.rs` writes them: timestamps in its
//! fixed-width form, `pending` as a boolean, `expires_ts` as the comparable
//! expiry key, optional columns left out when NULL. The driver
//! ([`crate::storage::local`]) writes them and then renames both tables to
//! `_legacy_<name>`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde_json::{json, Map, Value};

use super::super::store_documents::{expiry_key, rfc3339, trust_id, APPROVALS, FLOW_TRUST};
use crate::config::Config;
use crate::storage::local::{table_exists, ImportDoc};

/// The legacy tables this import retires.
pub(crate) const TABLES: &[&str] = &["pending_approvals", "flow_tool_trust"];

/// Reads every legacy row, after bringing an old schema forward.
pub(crate) fn read(config: &Config) -> Result<Vec<ImportDoc>> {
    super::with_connection(config, read_rows)
}

fn instant(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// `raw` in the document layout's fixed-width form, or unchanged when it is
/// not a timestamp (kept rather than dropped, so nothing is lost).
fn normalized(raw: &str) -> String {
    instant(raw).map_or_else(|| raw.to_string(), rfc3339)
}

fn read_rows(conn: &Connection) -> Result<Vec<ImportDoc>> {
    let mut docs = Vec::new();
    if table_exists(conn, "pending_approvals")? {
        let mut stmt = conn
            .prepare(
                "SELECT request_id, tool_name, action_summary, args_redacted, session_id,
                        created_at, expires_at, decided_at, decision, executed_at,
                        execution_outcome, execution_error, source_context,
                        tool_call_id, agent_id
                 FROM pending_approvals ORDER BY rowid",
            )
            .context("[approval::import] prepare pending_approvals")?;
        let rows = stmt
            .query_map([], |row| {
                let text = |i: usize| row.get::<_, Option<String>>(i);
                let mut doc = Map::new();
                let created = row.get::<_, String>(5)?;
                let expires = text(6)?;
                let decided = text(7)?;
                doc.insert("tool_name".into(), json!(row.get::<_, String>(1)?));
                doc.insert("action_summary".into(), json!(row.get::<_, String>(2)?));
                doc.insert("args_redacted".into(), json!(row.get::<_, String>(3)?));
                doc.insert("session_id".into(), json!(row.get::<_, String>(4)?));
                doc.insert("created_at".into(), json!(normalized(&created)));
                if let Some(expires) = &expires {
                    doc.insert("expires_at".into(), json!(normalized(expires)));
                    if let Some(at) = instant(expires) {
                        doc.insert("expires_ts".into(), json!(expiry_key(at)));
                    }
                }
                doc.insert("pending".into(), json!(decided.is_none()));
                for (field, value) in [
                    ("decided_at", decided.as_deref().map(normalized)),
                    ("decision", text(8)?),
                    ("executed_at", text(9)?.as_deref().map(normalized)),
                    ("execution_outcome", text(10)?),
                    ("execution_error", text(11)?),
                    ("source_context", text(12)?),
                    ("tool_call_id", text(13)?),
                    ("agent_id", text(14)?),
                ] {
                    if let Some(value) = value {
                        doc.insert(field.into(), json!(value));
                    }
                }
                Ok(ImportDoc {
                    collection: APPROVALS,
                    id: row.get(0)?,
                    doc: Value::Object(doc),
                })
            })
            .context("[approval::import] query pending_approvals")?;
        for row in rows {
            docs.push(row.context("[approval::import] decode a pending_approvals row")?);
        }
    }
    if table_exists(conn, "flow_tool_trust")? {
        let mut stmt = conn
            .prepare("SELECT flow_id, tool_name, created_at FROM flow_tool_trust ORDER BY rowid")
            .context("[approval::import] prepare flow_tool_trust")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .context("[approval::import] query flow_tool_trust")?;
        for row in rows {
            let (flow_id, tool_name, created) =
                row.context("[approval::import] decode a flow_tool_trust row")?;
            docs.push(ImportDoc {
                collection: FLOW_TRUST,
                id: trust_id(&flow_id, &tool_name),
                doc: json!({
                    "flow_id": flow_id,
                    "tool_name": tool_name,
                    "created_at": normalized(&created),
                }),
            });
        }
    }
    Ok(docs)
}

#[cfg(all(test, feature = "storage-sqlite"))]
#[path = "store_import_tests.rs"]
mod tests;
