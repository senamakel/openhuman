//! Sync audit log, per-source cost estimation, and the monthly cost summary.

use super::coding_sessions::unserved;
use crate::config::rpc as config_rpc;
use crate::memory::api::provider::sync::SyncAuditEntry;
use crate::memory::sources::readers;
use crate::memory::sources::registry;
use crate::rpc::RpcOutcome;

// ── Sync Audit Log ──

#[derive(Debug, serde::Serialize)]
pub struct SyncAuditLogResponse {
    pub entries: Vec<SyncAuditEntry>,
}

/// Past sync runs, newest first.
///
/// # This is now the driver's most recent rows, not the whole log
///
/// The engine call this replaced returned every line in
/// `<workspace>/memory_tree/sync_audit.jsonl`. `sync_audit_log` is capped:
/// `None` means "the driver's own ceiling", explicitly **not** unbounded,
/// because the log is append-only for the life of a workspace and an unbounded
/// read eventually cannot cross a frame at all. The panel that renders this
/// shows recent runs, so the cap is not a visible reduction there — but it is a
/// real one, and `monthly_cost_summary_rpc` below is where it has to be said
/// out loud rather than absorbed.
///
/// A read failure is now an error rather than an empty log. The engine wrapper
/// ended in `unwrap_or_default()`, so an unreadable file was reported as "no
/// syncs have run" — the one answer a caller cannot distinguish from the truth.
pub async fn sync_audit_log_rpc() -> Result<RpcOutcome<SyncAuditLogResponse>, String> {
    tracing::debug!("[memory_sources] sync_audit_log_rpc: entry");
    let config = config_rpc::load_config_with_timeout().await?;
    let binding = crate::memory::binding::for_config(&config)?;
    let Some(sync) = binding.provider().as_source_sync() else {
        return Err(unserved(&binding, "source sync", "sync_audit_log"));
    };

    // `None` = the driver's own cap. A caller cannot raise it by asking for
    // more, so passing a number here would only be this host inventing a
    // ceiling the driver then clamps anyway.
    let entries = sync
        .sync_audit_log(None)
        .await
        .map_err(|error| format!("sync audit log: {error}"))?;

    tracing::debug!(
        driver = %binding.driver_id(),
        entries = entries.len(),
        "[memory_sources] sync_audit_log_rpc: exit"
    );
    Ok(RpcOutcome::new(SyncAuditLogResponse { entries }, vec![]))
}

// ── Estimate Sync Cost ──

#[derive(Debug, serde::Deserialize)]
pub struct EstimateSyncCostRequest {
    pub source_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct EstimateSyncCostResponse {
    pub source_id: String,
    pub item_count: u32,
    pub estimated_tokens: u64,
    pub estimated_cost_usd: f64,
    pub budget_max_cost_usd: Option<f64>,
    pub budget_max_tokens: Option<u64>,
}

/// Project what syncing one source would cost.
///
/// The item count and the token estimate are this host's (they come from the
/// reader's listing and from the per-item allowances below); the **price** is
/// the driver's, asked through `estimate_sync_cost_usd`. That split is the
/// whole point of the member — see the module docs. A driver that serves no
/// sync family has no price to quote, and this refuses rather than quoting
/// `0.0`, which would read as "syncing this is free".
pub async fn estimate_sync_cost_rpc(
    req: EstimateSyncCostRequest,
) -> Result<RpcOutcome<EstimateSyncCostResponse>, String> {
    tracing::debug!(source_id = %req.source_id, "[memory_sources] estimate_sync_cost_rpc: entry");

    let source = registry::get_source(&req.source_id)
        .await?
        .ok_or_else(|| format!("source '{}' not found", req.source_id))?;

    let config = config_rpc::load_config_with_timeout().await?;
    let binding = crate::memory::binding::for_config(&config)?;
    let Some(sync) = binding.provider().as_source_sync() else {
        return Err(unserved(&binding, "source sync", "estimate_sync_cost"));
    };

    let reader = readers::reader_for(&source.kind);
    let items = reader.list_items(&source, &config).await?;

    let item_count = items.len() as u32;
    // estimated_tokens includes both input (500/item) and output (100/item)
    // to be consistent with the cost calculation below.
    let estimated_input_tokens = item_count as u64 * 500;
    let estimated_output_tokens = item_count as u64 * 100;
    let estimated_tokens = estimated_input_tokens + estimated_output_tokens;
    let estimated_cost_usd = sync
        .estimate_sync_cost_usd(estimated_input_tokens, estimated_output_tokens)
        .await
        .map_err(|error| format!("estimate sync cost: {error}"))?;

    tracing::debug!(
        driver = %binding.driver_id(),
        source_id = %req.source_id,
        item_count,
        estimated_tokens,
        estimated_cost_usd,
        "[memory_sources] estimate_sync_cost_rpc: exit"
    );
    Ok(RpcOutcome::new(
        EstimateSyncCostResponse {
            source_id: req.source_id,
            item_count,
            estimated_tokens,
            estimated_cost_usd,
            budget_max_cost_usd: source.max_cost_per_sync_usd,
            budget_max_tokens: source.max_tokens_per_sync,
        },
        vec![],
    ))
}

// ── Monthly Cost Summary ──

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MonthlyCostSummaryResponse {
    pub month: String,
    pub total_cost_usd: f64,
    pub total_syncs: u32,
    pub total_items: u32,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    /// Whether the audit read reached back past the start of `month`.
    ///
    /// `sync_audit_log` is capped by the driver, so a workspace that synced
    /// more times this month than the cap allows would silently total only the
    /// newest of them. `true` means at least one row *older* than `month` came
    /// back, which proves every row inside `month` was in the read; `false`
    /// means the read ran out first and the totals above are a **floor**.
    ///
    /// Deliberately conservative in one direction: a driver that simply has no
    /// older rows also reports `false`, so this can say "possibly short" when
    /// the totals are in fact exact. It never says "complete" when they are
    /// not, which is the direction that matters for a money figure.
    pub totals_complete: bool,
}

/// Total one month of audit rows.
///
/// Pure, so the cap-versus-boundary rule above is unit-testable without a
/// driver. Order-independent on purpose: the contract promises newest-first and
/// stopping at the first older row would be cheaper, but the list is already
/// bounded by the driver's cap, and a scan that does not depend on the ordering
/// cannot silently under-count if a driver ever returns rows out of order.
///
/// `pub(super)` so `rpc`'s sibling test module can reach it through
/// `super::*`.
pub(super) fn summarise_month(
    entries: &[SyncAuditEntry],
    month: &str,
) -> MonthlyCostSummaryResponse {
    let mut summary = MonthlyCostSummaryResponse {
        month: month.to_string(),
        total_cost_usd: 0.0,
        total_syncs: 0,
        total_items: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        // An empty log has no rows the cap could have hidden.
        totals_complete: entries.is_empty(),
    };

    for entry in entries {
        let entry_month = entry.timestamp.format("%Y-%m").to_string();
        // `%Y-%m` is zero-padded and fixed-width, so lexicographic order is
        // chronological order and this needs no date arithmetic.
        if entry_month.as_str() < month {
            summary.totals_complete = true;
            continue;
        }
        // Not `else` — a row stamped in a *later* month (clock skew) is skipped
        // rather than counted, exactly as the engine-backed filter did.
        if entry_month != month {
            continue;
        }
        summary.total_cost_usd += entry.effective_cost_usd();
        // Saturating: the counters are `u32` on the wire and an overflow here
        // would be a debug-build panic inside a read-only reporting RPC.
        summary.total_syncs = summary.total_syncs.saturating_add(1);
        summary.total_items = summary.total_items.saturating_add(entry.items_fetched);
        summary.total_input_tokens = summary
            .total_input_tokens
            .saturating_add(entry.input_tokens);
        summary.total_output_tokens = summary
            .total_output_tokens
            .saturating_add(entry.output_tokens);
    }

    summary
}

pub async fn monthly_cost_summary_rpc() -> Result<RpcOutcome<MonthlyCostSummaryResponse>, String> {
    tracing::debug!("[memory_sources] monthly_cost_summary_rpc: entry");
    let config = config_rpc::load_config_with_timeout().await?;
    let binding = crate::memory::binding::for_config(&config)?;
    let Some(sync) = binding.provider().as_source_sync() else {
        return Err(unserved(&binding, "source sync", "monthly_cost_summary"));
    };

    let entries = sync
        .sync_audit_log(None)
        .await
        .map_err(|error| format!("sync audit log: {error}"))?;

    let month = chrono::Utc::now().format("%Y-%m").to_string();
    let summary = summarise_month(&entries, &month);

    tracing::debug!(
        driver = %binding.driver_id(),
        month = %summary.month,
        rows_read = entries.len(),
        syncs = summary.total_syncs,
        totals_complete = summary.totals_complete,
        "[memory_sources] monthly_cost_summary_rpc: exit"
    );
    Ok(RpcOutcome::new(summary, vec![]))
}
