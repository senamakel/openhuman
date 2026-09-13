//! The "apply all in" sweep: enable every source, clear caps, and trigger a
//! sync for each.

use super::source_sync::{sync_dispatch, SyncDispatch};
use crate::config::rpc as config_rpc;
use crate::memory::sources::registry;
use crate::memory::sources::types::MemorySourceEntry;
use crate::rpc::RpcOutcome;

/// Response returned by `memory_sources_apply_all_in`.
#[derive(Debug, serde::Serialize)]
pub struct AllInResponse {
    /// All memory source entries after the "all in" transformation
    /// (every source enabled, every cap cleared).
    pub sources: Vec<MemorySourceEntry>,
    /// Number of sync tasks spawned (one per enabled source).
    pub sync_triggered: u32,
    /// Number of enabled sources whose sync trigger FAILED (openhuman#5820).
    ///
    /// Additive: an older caller reading only `sync_triggered` behaves as
    /// before, but a total failure no longer looks like a quiet 200 — in the
    /// incident, every source failed `no memory source registered` and the
    /// response still read as success with `sync_triggered: 0`.
    #[serde(default)]
    pub sync_failed: u32,
    /// One `"<source_id>: <error>"` line per failed trigger, in sweep order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sync_errors: Vec<String>,
}

/// The sweep half of [`apply_all_in_rpc`]: trigger a sync for every enabled
/// source, aggregating failures instead of laundering them (openhuman#5820 —
/// in the incident every trigger failed `no memory source registered` and the
/// RPC still answered a clean success).
///
/// Takes the trigger as a closure rather than the driver trait object so the
/// aggregation is unit-testable without a full `MemorySourceSync` stub; the
/// RPC owns config/binding resolution and the response shape.
///
/// The closure receives the whole entry, not just the id: since openhuman#6007
/// the caller has to route each row by [`SyncDispatch`], which reads the kind,
/// the connection and the per-source cap. Owned rather than borrowed so the
/// returned future does not have to borrow from the sweep's loop body.
///
/// `pub(super)` so `rpc`'s sibling test module can reach it through
/// `super::*`.
pub(super) async fn trigger_enabled_syncs<F, Fut>(
    sources: &[MemorySourceEntry],
    mut trigger: F,
) -> (u32, Vec<String>)
where
    F: FnMut(MemorySourceEntry) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut sync_triggered: u32 = 0;
    let mut sync_errors: Vec<String> = Vec::new();
    for source in sources {
        if !source.enabled {
            continue;
        }
        tracing::debug!(
            source_id = %source.id,
            kind = %source.kind.as_str(),
            "[memory_sources] apply_all_in_rpc: triggering sync"
        );
        match trigger(source.clone()).await {
            Ok(()) => {
                sync_triggered += 1;
            }
            Err(e) => {
                // Per-source failure stays non-fatal for the sweep, but it is
                // AGGREGATED into the response rather than laundered into a
                // clean 200.
                tracing::warn!(
                    source_id = %source.id,
                    error = %e,
                    "[memory_sources] apply_all_in_rpc: sync trigger failed for source"
                );
                sync_errors.push(format!("{}: {e}", source.id));
            }
        }
    }
    (sync_triggered, sync_errors)
}

/// Enable ALL memory sources, clear all caps, and trigger a sync for
/// every source.
///
/// Returns immediately with the updated source list and the number of
/// syncs queued. Individual syncs run in the background and publish
/// `MemorySyncStageChanged` events as they progress.
pub async fn apply_all_in_rpc() -> Result<RpcOutcome<AllInResponse>, String> {
    tracing::info!("[memory_sources] apply_all_in_rpc: entry");

    // Enable all sources and clear caps.
    let sources = registry::apply_all_in().await?;

    // Trigger a background sync for every enabled source.
    let config = config_rpc::load_config_with_timeout().await?;

    let binding = crate::memory::binding::for_config(&config)?;
    // Resolved once for the whole sweep, but as an `Option` rather than a hard
    // error. A driver that serves `as_sources` without `as_source_sync` can
    // still sync every Composio row through the connector, and failing the whole
    // sweep over a capability those rows never use is the review finding the
    // row-level path already carries (#5932) — for a Composio-only user it
    // turned "sync everything" into one flat refusal. A row that genuinely needs
    // the family reports the refusal as its own per-source error, which is
    // exactly what this sweep aggregates.
    let source_sync = binding.provider().as_source_sync();
    let config_ref = &config;
    let driver_id = binding.driver_id();

    let (sync_triggered, sync_errors) = trigger_enabled_syncs(&sources, move |source| async move {
        match sync_dispatch(&source)? {
            // The connector-backed run IS the sync for this kind, so the sweep
            // takes the same dispatch the per-row Sync button does. Before
            // openhuman#6007 every Composio row came back "synced through the
            // connector module, not this engine" here.
            SyncDispatch::Connector {
                connection_id,
                max_items,
            } => crate::integrations::composio::ops::composio_sync_budgeted(
                config_ref,
                &connection_id,
                // `manual`, not a sweep-specific reason: Apply-all is a user
                // action, and `parse_sync_reason` accepts only `manual`,
                // `periodic` and `connection_created` — an invented reason would
                // fail every Composio row here with "unrecognized sync reason".
                Some("manual".to_string()),
                Some(source.id.clone()),
                max_items,
            )
            .await
            .map(|_| ()),
            SyncDispatch::Driver => {
                let sync = source_sync.ok_or_else(|| {
                    format!("the bound memory driver '{driver_id}' does not serve source sync")
                })?;
                sync.run_source_sync(&source.id)
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
    })
    .await;

    let sync_failed = sync_errors.len() as u32;
    if sync_failed > 0 && sync_triggered == 0 {
        // Every enabled source failed to trigger — that is a broken sweep,
        // not a best-effort one. Log at ERROR so it cannot hide at warn among
        // the per-source lines.
        tracing::error!(
            sources = sources.len(),
            sync_failed,
            "[memory_sources] apply_all_in_rpc: every sync trigger failed"
        );
    }

    tracing::info!(
        sources = sources.len(),
        sync_triggered,
        sync_failed,
        "[memory_sources] apply_all_in_rpc: complete"
    );

    Ok(RpcOutcome::new(
        AllInResponse {
            sources,
            sync_triggered,
            sync_failed,
            sync_errors,
        },
        vec![],
    ))
}
