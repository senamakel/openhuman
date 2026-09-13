//! Driver diagnostics: aggregate store/queue stats and the re-embed backfill
//! status.
//!
//! The numbers below used to come from `SELECT`s against TinyCortex's tables.
//! They come from the bound driver now, which is what lets a workspace run on
//! a driver that is not TinyCortex and still answer "how far behind is the
//! pipeline".

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::rpc::RpcOutcome;

/// The driver's identifier for a re-embed backfill job.
///
/// Job kinds are the driver's own vocabulary, not the contract's — a driver
/// that never enqueues this one answers zero for it, which is the honest
/// count and exactly what a status poll wants to hear.
const REEMBED_BACKFILL_KIND: &str = "reembed_backfill";

/// Aggregate counts over the bound driver's stored chunks.
///
/// Zeroed rather than refused when the driver does not serve `Maintenance`:
/// this feeds a status surface, and a status surface that errors tells the
/// user less than one reporting an empty store.
///
/// `pub(super)` — shared with [`super::pipeline_status`].
pub(super) async fn store_stats(
    config: &Config,
) -> Result<crate::memory::api::provider::types::StoreStats, String> {
    let binding = crate::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] store_stats: driver '{}' does not serve Maintenance; reporting empty",
            binding.driver_id()
        );
        return Ok(Default::default());
    };
    maintenance
        .store_stats()
        .await
        .map_err(|e| format!("store_stats: {e}"))
}

/// The bound driver's queue state, optionally narrowed to one job kind.
///
/// A driver error propagates, the same way [`store_stats`] propagates its own.
/// That matters most for `backfill_status_rpc`, which is asked whether a modal
/// may close: guessing "nothing pending" would dismiss it over a live
/// backfill. A driver that does not serve `Maintenance` still reports empty
/// rather than erroring, because it has no queue to be behind on.
///
/// `pub(super)` — shared with [`super::pipeline_status`].
pub(super) async fn queue_stats(
    config: &Config,
    kind: Option<&str>,
) -> Result<crate::memory::api::provider::types::QueueStats, String> {
    let binding = crate::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] queue_stats: driver '{}' does not serve Maintenance; reporting empty",
            binding.driver_id()
        );
        return Ok(Default::default());
    };
    maintenance
        .queue_stats(kind)
        .await
        .map_err(|e| format!("queue_stats: {e}"))
}

/// Whether the driver has a re-embed backfill chain running.
///
/// Scoped to the driver's process, not to this store — the contract member says
/// so in its own signature, which is why it is a member rather than a field on
/// [`queue_stats`]. A driver serving several stores answers the same for all of
/// them.
///
/// A driver without Maintenance reports `false` rather than erroring, matching
/// its siblings: "this driver runs no backfill" is true of it, not a fault the
/// caller can act on.
///
/// `pub(super)` — reused verbatim by tests declared directly under `rpc`.
pub(super) async fn backfill_in_progress(config: &Config) -> Result<bool, String> {
    let binding = crate::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] backfill_in_progress: driver '{}' does not serve Maintenance; reporting false",
            binding.driver_id()
        );
        return Ok(false);
    };
    maintenance
        .backfill_in_progress()
        .await
        .map_err(|e| format!("backfill_in_progress: {e}"))
}

/// Response from the `memory_backfill_status` RPC (#1574 §4b). The frontend
/// polls this while the re-embed modal is open to surface progress and to
/// dismiss the modal once the new embedding space is fully covered.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackfillStatusResponse {
    /// True while a re-embed backfill chain still has work pending — the
    /// #1365 flag OR a queued/running `reembed_backfill` job.
    pub in_progress: bool,
    /// Count of `reembed_backfill` jobs in `ready` or `running` state. `0`
    /// with `in_progress=false` means the active embedding space is fully
    /// covered (modal can close).
    pub pending_jobs: u64,
}

/// `memory_backfill_status` RPC handler (#1574 §4b). No inputs — reports
/// whether a per-model re-embed backfill is in flight so the UI can warn
/// the user that semantic recall is reduced until it drains.
pub async fn backfill_status_rpc(
    config: &Config,
) -> Result<RpcOutcome<BackfillStatusResponse>, String> {
    log::debug!("[memory::rpc] backfill_status: entry");
    // Asked of the bound driver rather than of TinyCortex's tables. No
    // `spawn_blocking` here any more: the driver owns whether its own reads
    // block, and a host that wraps them a second time is guessing about
    // storage it no longer talks to.
    let queue = queue_stats(config, Some(REEMBED_BACKFILL_KIND))
        .await
        .map_err(|e| {
            let msg = format!("memory_backfill_status: {e}");
            log::debug!("[memory::rpc] backfill_status: error: {msg}");
            msg
        })?;
    // Ready + running, not `total - done`: a failed backfill job is finished
    // with, and counting it as pending leaves the modal open forever.
    let pending_jobs: u64 = queue.ready + queue.running;
    // Asked of the driver, not of the host-linked engine's process-global. That
    // static covers the instant between one backfill link settling and the next
    // being enqueued, which the counts cannot see — but re-embedding runs in the
    // module now, and a `cdylib` has its own statics, so the host-side copy reads
    // `false` forever and the modal closes while work is still being prepared.
    //
    // The member is process-wide rather than store-scoped, and says so in its
    // signature; that is why it is not a `QueueStats` field, where a per-store
    // snapshot would have implied a scoping it does not have. A read failure
    // degrades to the counts rather than failing the polled RPC.
    let driver_backfilling = backfill_in_progress(config).await.unwrap_or_else(|e| {
        log::warn!("[memory::rpc] backfill_status: backfill_in_progress read failed: {e}");
        false
    });
    let in_progress = driver_backfilling || pending_jobs > 0;
    Ok(RpcOutcome::single_log(
        BackfillStatusResponse {
            in_progress,
            pending_jobs,
        },
        format!("memory_tree: backfill_status in_progress={in_progress} pending={pending_jobs}"),
    ))
}
