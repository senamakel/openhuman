//! Flow-trust persistence split out of `store.rs` (see that file's module
//! doc) purely to keep it under the repo's per-file line budget.
//!
//! Covers the save-time flow pre-authorization audit row plus the
//! `flow_tool_trust` table: per-`(flow_id, tool_name)` "approve always for
//! this flow" grants used by `ApprovalGate::intercept_audited` to
//! short-circuit parking for a trusted flow/tool pair.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::params;

use crate::config::Config;

use super::super::types::{ApprovalDecision, ApprovalSourceContext};
use super::with_connection;

/// The action summary of a flow pre-authorization audit row.
const PREAUTHORIZATION_SUMMARY: &str = "Pre-authorized for this flow when it was saved and enabled";

/// Record a save-time flow pre-authorization in the durable audit trail as a
/// born-decided row (`decided_at = created_at`, decision
/// `approve_always_for_flow`): it never appears in `list_pending` (which
/// filters `decided_at IS NULL`) but does surface in
/// `list_recent_decisions`, so Settings → Approval history shows exactly
/// when and for which tool the user granted blanket trust. The
/// `source_context` carries an empty `run_id` — no run existed yet — which
/// also keeps it invisible to `list_pending_for_flow_run`.
pub fn record_flow_preauthorization(
    config: &Config,
    flow_id: &str,
    tool_name: &str,
    session_id: &str,
) -> Result<()> {
    if let Some(docs) = super::super::store_documents::current(config)? {
        return docs.insert_decided(
            tool_name,
            PREAUTHORIZATION_SUMMARY,
            session_id,
            &ApprovalSourceContext::Flow {
                flow_id: flow_id.to_string(),
                run_id: String::new(),
                node_id: None,
            },
            ApprovalDecision::ApproveAlwaysForFlow,
        );
    }
    with_connection(config, |conn| {
        let now = Utc::now().to_rfc3339();
        let source_context = serde_json::to_string(&ApprovalSourceContext::Flow {
            flow_id: flow_id.to_string(),
            run_id: String::new(),
            node_id: None,
        })
        .context("[approval::store] serialize preauthorization source_context")?;
        conn.execute(
            "INSERT INTO pending_approvals
                (request_id, tool_name, action_summary, args_redacted,
                 session_id, created_at, expires_at, source_context,
                 decided_at, decision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?6, ?8)",
            params![
                uuid::Uuid::new_v4().to_string(),
                tool_name,
                PREAUTHORIZATION_SUMMARY,
                "{}",
                session_id,
                now,
                source_context,
                ApprovalDecision::ApproveAlwaysForFlow.as_str(),
            ],
        )
        .context("[approval::store] insert preauthorization audit row")?;
        Ok(())
    })
}

/// Grant "approve always for this flow" trust to a `(flow_id, tool_name)`
/// pair — inserted when the user picks `ApproveAlwaysForFlow` on a
/// flow-origin park. `INSERT OR IGNORE` makes re-granting an already-trusted
/// pair a harmless no-op rather than a primary-key error.
pub fn insert_flow_trust(config: &Config, flow_id: &str, tool_name: &str) -> Result<()> {
    if let Some(docs) = super::super::store_documents::current(config)? {
        return docs.insert_flow_trust(flow_id, tool_name);
    }
    with_connection(config, |conn| {
        conn.execute(
            "INSERT OR IGNORE INTO flow_tool_trust (flow_id, tool_name, created_at)
             VALUES (?1, ?2, ?3)",
            params![flow_id, tool_name, Utc::now().to_rfc3339()],
        )
        .context("[approval::store] insert_flow_trust")?;
        Ok(())
    })
}

/// List every `tool_name` currently holding "approve always for this flow"
/// trust for `flow_id`, ordered by name for stable output. Used by the
/// save-time pre-authorization manifest (`flows_approval_manifest`) to diff
/// "what the graph needs" against "what is already granted".
pub fn list_flow_trust(config: &Config, flow_id: &str) -> Result<Vec<String>> {
    if let Some(docs) = super::super::store_documents::current(config)? {
        return docs.list_flow_trust(flow_id);
    }
    with_connection(config, |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT tool_name FROM flow_tool_trust
                 WHERE flow_id = ?1 ORDER BY tool_name",
            )
            .context("[approval::store] list_flow_trust prepare")?;
        let names = stmt
            .query_map(params![flow_id], |row| row.get::<_, String>(0))
            .context("[approval::store] list_flow_trust query")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("[approval::store] list_flow_trust rows")?;
        Ok(names)
    })
}

/// Delete flow trust rows for `flow_id`. With `tool_names: None` every grant
/// for the flow is removed (flow deletion cleanup); with `Some(names)` only
/// the named grants are revoked. Returns the number of rows removed. Deleting
/// a name that was never granted is a no-op, keeping the call idempotent.
pub fn delete_flow_trust(
    config: &Config,
    flow_id: &str,
    tool_names: Option<&[String]>,
) -> Result<usize> {
    if let Some(docs) = super::super::store_documents::current(config)? {
        return docs.delete_flow_trust(flow_id, tool_names);
    }
    with_connection(config, |conn| {
        let removed = match tool_names {
            None => conn
                .execute(
                    "DELETE FROM flow_tool_trust WHERE flow_id = ?1",
                    params![flow_id],
                )
                .context("[approval::store] delete_flow_trust all")?,
            Some(names) => {
                let mut removed = 0usize;
                for name in names {
                    removed += conn
                        .execute(
                            "DELETE FROM flow_tool_trust
                             WHERE flow_id = ?1 AND tool_name = ?2",
                            params![flow_id, name],
                        )
                        .context("[approval::store] delete_flow_trust named")?;
                }
                removed
            }
        };
        Ok(removed)
    })
}

/// Whether `(flow_id, tool_name)` was previously granted "approve always for
/// this flow" trust. Consulted by [`super::gate::ApprovalGate::intercept_audited`]
/// before parking a `Workflow`-origin tool call.
pub fn is_flow_tool_trusted(config: &Config, flow_id: &str, tool_name: &str) -> Result<bool> {
    if let Some(docs) = super::super::store_documents::current(config)? {
        return docs.is_flow_tool_trusted(flow_id, tool_name);
    }
    with_connection(config, |conn| {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM flow_tool_trust WHERE flow_id = ?1 AND tool_name = ?2
                 )",
                params![flow_id, tool_name],
                |row| row.get(0),
            )
            .context("[approval::store] is_flow_tool_trusted")?;
        Ok(exists)
    })
}
