//! A driver-backed source sync that reports how it ended (openhuman#6257).
//!
//! The Sync button and Apply all hand folder, GitHub, RSS and web sources to
//! `MemorySourceSync::run_source_sync`. Before #5725 those rows went through the
//! engine's own `sync_source`, which published the run's start and finish and
//! wrote its audit row; the contract member does neither. So a manual sync of
//! these kinds finished in silence: the row stayed on the per-document
//! "Queued" stage until the activity tracker gave up on it, no result chip
//! appeared, the post-sync embed never started, and Sync History never gained a
//! row. [`run_recorded`] puts all of that back around the call.

use std::future::Future;
use std::time::Instant;

use crate::config::Config;
use crate::core::events::DomainEvent;
use crate::memory::api::error::MemoryError;
use crate::memory::api::provider::sync::SyncRunOutcome;
use crate::memory::sources::run_history::{self, HostRun};
use crate::memory::sources::types::MemorySourceEntry;

/// The trigger a user-started driver run reports under — the word the engine's
/// `sync_source` used for the Sync button.
const TRIGGER: &str = "manual";

/// The scope a history row names for `entry`: the rule the engine's periodic
/// writer applies (URL, then toolkit, then the id), so a source's manual and
/// scheduled rows read alike.
pub(super) fn history_scope(entry: &MemorySourceEntry) -> String {
    entry
        .url
        .clone()
        .or_else(|| entry.toolkit.clone())
        .unwrap_or_else(|| entry.id.clone())
}

/// The stage event for one driver-backed source.
pub(super) fn stage_event(
    source_id: &str,
    kind: &str,
    stage: &str,
    detail: Option<String>,
) -> DomainEvent {
    DomainEvent::MemorySyncStageChanged {
        trigger: TRIGGER.to_string(),
        stage: stage.to_string(),
        provider: Some(kind.to_string()),
        // The row id here too, as the engine's `sync_source` sent it: the app
        // still falls back to `connection_id` for a core that sends no
        // `source_id`.
        connection_id: Some(source_id.to_string()),
        detail,
        source_id: Some(source_id.to_string()),
    }
}

/// A stage publisher for one source, on the process bus.
pub(super) fn bus_stage_publisher(source_id: &str, kind: &str) -> impl Fn(&str, Option<String>) {
    let source_id = source_id.to_string();
    let kind = kind.to_string();
    move |stage, detail| {
        crate::core::bus::BUS.publish(stage_event(&source_id, &kind, stage, detail));
    }
}

/// Run one driver-backed sync, publishing its start and finish and recording
/// it in the host's run log.
///
/// `run` performs the sync, `describe` turns its failure into the text the
/// caller returns, and `publish` sends one stage for the source. All three are
/// parameters so the sequence is testable without a driver or the bus.
///
/// The row is written before the terminal stage is published, on purpose: the
/// history panel refetches when that stage arrives, and a row written after it
/// would miss that read.
pub(super) async fn run_recorded<Run, Fut, Describe, Publish>(
    config: &Config,
    source_id: &str,
    entry: Option<&MemorySourceEntry>,
    run: Run,
    describe: Describe,
    publish: Publish,
) -> Result<SyncRunOutcome, String>
where
    Run: FnOnce() -> Fut,
    Fut: Future<Output = Result<SyncRunOutcome, MemoryError>>,
    Describe: FnOnce(&MemoryError) -> String,
    Publish: Fn(&str, Option<String>),
{
    let source_kind = entry.map_or("unknown", |entry| entry.kind.as_str());
    tracing::debug!(
        source_id = %source_id,
        kind = source_kind,
        "[memory_sources:driver_run] run starting"
    );
    publish("running", None);
    let started = Instant::now();
    let result = run().await;

    let record = |outcome: Option<&SyncRunOutcome>, error: Option<String>| {
        run_history::record_run(
            config,
            HostRun {
                source_id: source_id.to_string(),
                source_kind: source_kind.to_string(),
                scope: entry.map_or_else(|| source_id.to_string(), history_scope),
                items: outcome.map_or(0, |outcome| u64::from(outcome.records_ingested)),
                duration_ms: run_history::elapsed_ms(started),
                error,
                actions_called: outcome.map_or(0, |outcome| outcome.actions_called),
                provider_cost_usd: outcome.map_or(0.0, |outcome| outcome.provider_cost_usd),
            }
            .into_entry(chrono::Utc::now()),
        );
    };

    match result {
        Ok(outcome) => {
            record(Some(&outcome), None);
            tracing::debug!(
                source_id = %source_id,
                items = outcome.records_ingested,
                more_pending = outcome.more_pending,
                "[memory_sources:driver_run] run completed"
            );
            publish(
                "completed",
                Some(run_history::completed_sync_detail(
                    u64::from(outcome.records_ingested),
                    outcome.more_pending,
                    outcome.note.as_deref(),
                )),
            );
            Ok(outcome)
        }
        Err(error) => {
            let message = describe(&error);
            record(None, Some(message.clone()));
            tracing::warn!(
                source_id = %source_id,
                error = %message,
                "[memory_sources:driver_run] run failed"
            );
            publish("failed", Some(message.clone()));
            Err(message)
        }
    }
}

#[cfg(test)]
#[path = "driver_run_tests.rs"]
mod tests;
