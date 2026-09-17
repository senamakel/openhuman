//! Per-source ingest status and the supported-toolkit catalog.

use crate::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

// ── Status List ──

#[derive(Debug, serde::Serialize)]
pub struct StatusListResponse {
    pub statuses: Vec<crate::memory::sources::status::SourceStatus>,
}

pub async fn status_list_rpc() -> Result<RpcOutcome<StatusListResponse>, String> {
    tracing::debug!("[memory_sources] status_list_rpc: entry");
    let config = config_rpc::load_config_with_timeout().await?;
    let statuses = crate::memory::sources::status::status_list(&config).await?;
    Ok(RpcOutcome::new(StatusListResponse { statuses }, vec![]))
}

// ── Supported Toolkits ──

#[derive(Debug, serde::Serialize)]
pub struct SupportedToolkitsResponse {
    /// Sorted, de-duplicated toolkit slugs that ship a native memory-sync
    /// provider (e.g. `clickup`, `github`, `gmail`, `linear`, `notion`,
    /// `slack`). Anything outside this set can never sync.
    pub toolkits: Vec<String>,
}

/// Toolkit slugs the memory-sync layer can actually run, sourced from
/// [`NATIVE_PROVIDERS`](crate::integrations::composio::providers::NATIVE_PROVIDERS)
/// — the single source of truth shared with `scan_active_sync_targets`
/// (via `has_native_provider`). Exposed so the Add Source picker can disable
/// connections whose toolkit has no provider instead of letting the user add
/// a dead source. See issue #3352.
///
/// Was sourced from the engine's provider registry
/// (`all_providers().iter().map(|p| p.toolkit_slug())`); tinymemory v1.13.4
/// deleted that registry along with the rest of the in-process pipeline.
/// `NATIVE_PROVIDERS` names the same six toolkits by construction — the
/// catalog was always kept in step with the registry it described — so no
/// registration step (`init_default_composio_sync_providers`) is needed any
/// more either: this is now a `&'static` table read, not a process-global
/// `HashMap` that has to be primed first.
pub async fn supported_toolkits_rpc() -> Result<RpcOutcome<SupportedToolkitsResponse>, String> {
    tracing::debug!("[memory_sources] supported_toolkits_rpc: entry");

    let mut toolkits: Vec<String> = crate::integrations::composio::providers::NATIVE_PROVIDERS
        .iter()
        .map(|(slug, _interval_secs)| (*slug).to_string())
        .collect();
    toolkits.sort();
    toolkits.dedup();

    tracing::debug!(
        count = toolkits.len(),
        toolkits = ?toolkits,
        "[memory_sources] supported_toolkits_rpc: resolved supported toolkit set"
    );
    Ok(RpcOutcome::new(
        SupportedToolkitsResponse { toolkits },
        vec![],
    ))
}
