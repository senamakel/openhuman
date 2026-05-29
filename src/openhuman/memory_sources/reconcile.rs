//! Startup reconciliation of Composio connections into the memory sources registry.
//!
//! Called once at boot to ensure all active Composio sync targets have
//! a corresponding `MemorySourceEntry` in config. This catches connections
//! created before the memory_sources domain existed.

use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::memory_sources::registry;
use crate::openhuman::memory_sync::composio;

pub async fn ensure_composio_sources() {
    tracing::debug!("[memory_sources:reconcile] starting composio reconciliation");

    let config = match config_rpc::load_config_with_timeout().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "[memory_sources:reconcile] failed to load config; skipping"
            );
            return;
        }
    };

    // Always hit Composio directly here — using list_sync_targets would
    // short-circuit through the registry and miss new connections.
    let targets = match composio::scan_active_sync_targets(&config).await {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!(
                error = %e,
                "[memory_sources:reconcile] no composio sync targets available; skipping"
            );
            return;
        }
    };

    let mut upserted = 0u32;
    for target in &targets {
        // Use a title-cased toolkit name plus the truncated connection id
        // so distinct Gmail accounts don't all show as "Gmail connection".
        let label = format!(
            "{} · {}",
            title_case(&target.toolkit),
            short_id(&target.connection_id)
        );
        match registry::upsert_composio_source(&target.toolkit, &target.connection_id, &label).await
        {
            Ok(_) => {
                upserted += 1;
            }
            Err(e) => {
                tracing::warn!(
                    toolkit = %target.toolkit,
                    connection_id = %target.connection_id,
                    error = %e,
                    "[memory_sources:reconcile] upsert failed"
                );
            }
        }
    }

    if !targets.is_empty() {
        tracing::info!(
            targets = targets.len(),
            upserted = upserted,
            "[memory_sources:reconcile] composio reconciliation complete"
        );
    }
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

fn short_id(id: &str) -> &str {
    // Show only the last 8 chars to keep labels compact.
    let take = id.len().saturating_sub(8);
    &id[take..]
}
