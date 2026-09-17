//! Sync History rows for Composio runs (openhuman#6257).
//!
//! Every Composio run is read by the connector module and ingested by this
//! host, so the memory driver's audit log never sees one — the engine loop that
//! used to write those rows left with tinymemory v1.13.4. They are recorded in
//! the host's run log instead, at the two places a run ends: the Sources row's
//! budgeted loop in `providers_ops`, and the single-call drain in `pass_budget`
//! that every other entry point shares.

use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::memory::api::provider::sync::SyncAuditEntry;
use crate::memory::sources::run_history::{self, HostRun};

use super::providers_ops::SOURCE_KIND;
use super::source_rows::source_id_for_connection;

/// The reason the periodic loop syncs under.
const PERIODIC: &str = "periodic";

/// Whether a finished run earns a Sync History row.
///
/// A run someone started always does — the Sync button, Apply all, the first
/// sync after connecting, a provider sync, Slack's own RPC — because "did my
/// sync happen" is the question the history answers. The periodic loop polls
/// every connection on a short interval, and a poll that found nothing new is
/// not a sync anyone asked about; recorded, it would bury the runs that were.
/// So a periodic run earns a row only when it wrote something or failed.
pub(crate) fn should_record(reason: &str, written: u64, failed: bool) -> bool {
    reason != PERIODIC || failed || written > 0
}

/// The scope a connector run files under: `{toolkit}:{connection_id}`, toolkit
/// lowercased, both trimmed — the ingest funnel's `path_scope`, so the history
/// row and the sealed tree name the same thing.
pub(crate) fn connector_scope(toolkit: &str, connection_id: &str) -> String {
    format!(
        "{}:{}",
        toolkit.trim().to_ascii_lowercase(),
        connection_id.trim()
    )
}

/// One finished connector run.
pub(crate) struct ConnectorRun<'a> {
    pub toolkit: &'a str,
    pub connection_id: &'a str,
    /// The Sources row that started the run, when one did; otherwise the row is
    /// looked up by connection.
    pub source_id: Option<&'a str>,
    pub reason: &'a str,
    pub started: Instant,
    /// Items the run wrote before it ended.
    pub written: u64,
    /// Why the run failed; `None` for a run that completed.
    pub error: Option<&'a str>,
}

/// The history row for `run`, naming `source_id` when the registry has one and
/// the connection's scope otherwise.
pub(crate) fn connector_run_entry(
    run: &ConnectorRun<'_>,
    source_id: Option<String>,
    finished_at: DateTime<Utc>,
) -> SyncAuditEntry {
    let scope = connector_scope(run.toolkit, run.connection_id);
    HostRun {
        source_id: source_id.unwrap_or_else(|| scope.clone()),
        source_kind: SOURCE_KIND.to_string(),
        scope,
        items: run.written,
        duration_ms: run_history::elapsed_ms(run.started),
        error: run.error.map(str::to_string),
        ..HostRun::default()
    }
    .into_entry(finished_at)
}

/// Record `run` in the host's run log, when [`should_record`] says it earns a
/// row.
pub(crate) fn record(config: &Config, run: &ConnectorRun<'_>) {
    if !should_record(run.reason, run.written, run.error.is_some()) {
        tracing::debug!(
            toolkit = %run.toolkit,
            connection_id = %run.connection_id,
            "[composio] periodic run found nothing new; no history row"
        );
        return;
    }
    let source_id = run
        .source_id
        .map(str::to_string)
        .or_else(|| source_id_for_connection(config, run.toolkit, run.connection_id));
    run_history::record_run(config, connector_run_entry(run, source_id, Utc::now()));
}

#[cfg(test)]
#[path = "connector_runs_tests.rs"]
mod tests;
