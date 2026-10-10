//! Approval rows by the embedded agent that parked them, and whether the
//! store exists to hold any.

use anyhow::{Context, Result};
use rusqlite::params;

use super::{Config, PendingApproval};

/// Whether the approval store has been created for `config`'s workspace.
/// A configured document store always exists.
pub fn exists(config: &Config) -> bool {
    crate::storage::installed().is_some() || super::db_path(config).is_file()
}

/// [`list_pending`](super::list_pending) narrowed to the rows of `agent` (see
/// [`PendingApproval::belongs_to`]).
pub fn list_pending_for_agent(
    config: &Config,
    agent: Option<&str>,
) -> Result<Vec<PendingApproval>> {
    Ok(super::list_pending(config)?
        .into_iter()
        .filter(|row| row.belongs_to(agent))
        .collect())
}

/// The agent that parked the still-undecided `request_id`: `Ok(None)` when no
/// such row exists, `Ok(Some(None))` for a row the process parked itself.
pub fn pending_agent(config: &Config, request_id: &str) -> Result<Option<Option<String>>> {
    if let Some(docs) = super::super::store_documents::current(config)? {
        return docs.pending_agent(request_id);
    }
    super::with_connection(config, |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT agent_id FROM pending_approvals
                 WHERE request_id = ?1 AND decided_at IS NULL",
            )
            .context("[approval::store] prepare pending_agent")?;
        let mut rows = stmt
            .query(params![request_id])
            .context("[approval::store] query pending_agent")?;
        match rows
            .next()
            .context("[approval::store] pending_agent next")?
        {
            Some(row) => Ok(Some(
                row.get::<_, Option<String>>(0)
                    .context("[approval::store] pending_agent decode")?,
            )),
            None => Ok(None),
        }
    })
}
