//! Per-source sync (the row-level Sync button) and raw-archive ↔ tree
//! reconciliation.

use super::coding_sessions::unserved;
use crate::config::rpc as config_rpc;
use crate::memory::sources::registry;
use crate::memory::sources::types::MemorySourceEntry;
use crate::rpc::RpcOutcome;

// ── Sync ──

#[derive(Debug, serde::Deserialize)]
pub struct SyncRequest {
    pub source_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SyncResponse {
    pub requested: bool,
    pub source_id: String,
}

/// Which sync path a source row belongs to.
///
/// Extracted because there are **two** callers and they disagreed. The per-row
/// Sync button ([`sync_rpc`]) special-cased Composio; the Apply-all sweep
/// ([`super::apply_all::apply_all_in_rpc`]) dispatched every enabled row
/// through `MemorySourceSync`, which the driver refuses for Composio ("… is
/// synced through the connector module, not this engine"). So "sync
/// everything" failed for exactly the rows a user most expects it to fix
/// (openhuman#6007). Two call sites open-coding one rule is how that
/// happened; both now match on this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SyncDispatch {
    /// Connector-mediated. tinymemory v1.13.4 removed the engine's in-process
    /// Composio sync, so the connector-backed run IS the sync for this kind —
    /// the same entry point the `openhuman.composio_sync` RPC uses, which reads
    /// the connected account through the module and ingests through this same
    /// binding.
    Connector {
        connection_id: String,
        max_items: Option<u32>,
    },
    /// The memory driver's own source pipeline — folder, git, RSS, and every
    /// other tree-coupled kind.
    Driver,
}

/// Resolve a source row to its sync path.
///
/// The only failure is a Composio row carrying no `connection_id`: the connector
/// call is addressed *by* connection, so there is nothing to sync and nothing to
/// guess. That is a malformed row rather than a transient fault, which is why the
/// message says to remove and re-add rather than to retry.
pub(crate) fn sync_dispatch(entry: &MemorySourceEntry) -> Result<SyncDispatch, String> {
    if entry.kind != tinymemory_sources::types::SourceKind::Composio {
        return Ok(SyncDispatch::Driver);
    }
    let connection_id = entry.connection_id.clone().ok_or_else(|| {
        format!(
            "composio source '{}' has no connection_id; remove and re-add the source",
            entry.id
        )
    })?;
    Ok(SyncDispatch::Connector {
        connection_id,
        max_items: entry.max_items,
    })
}

/// Turn a source-sync failure into something a reader can act on.
///
/// A `NotFound` from the driver is ambiguous: either nobody ever registered the
/// id, or the driver is reading a different registry than the host is writing.
/// The second happens because the memory module is loaded once per process and
/// tinybus never unloads a library (`modules/ops.rs`), so the `config_path` it
/// is handed at load is the one it keeps for the life of the process. Boot
/// signed out, log in, and the host starts writing `[[memory_sources]]` into
/// the new profile's `config.toml` while the module still reads the pre-login
/// one — every id the host registers is then unknown to it, permanently.
///
/// The host can tell the two apart, because at this point it holds both halves
/// of the contradiction: `host_has_source` is its own registry's answer for the
/// same id. When the host has it and the driver does not, the registries
/// disagree, and the only in-process remedy is a restart — the same remedy
/// `ops` already states for a module that failed to load.
///
/// Every other failure is returned exactly as the driver stated it; inventing a
/// diagnosis for those would send the reader down the wrong path.
///
/// `pub(super)` so `rpc`'s sibling test module can reach it through
/// `super::*`.
pub(super) fn describe_source_sync_failure(
    source_id: &str,
    host_has_source: bool,
    error: &crate::memory::api::error::MemoryError,
) -> String {
    match error {
        crate::memory::api::error::MemoryError::NotFound(_) if host_has_source => format!(
            "the memory module cannot see source '{source_id}', but this profile has it \
             registered. The module is bound to a different profile's source registry — it \
             reads the config it was given when it first loaded, and that binding cannot be \
             changed while the app is running. Restart the app to rebind it."
        ),
        other => other.to_string(),
    }
}

pub async fn sync_rpc(req: SyncRequest) -> Result<RpcOutcome<SyncResponse>, String> {
    tracing::info!(source_id = %req.source_id, "[memory_sources] sync_rpc: entry");

    let config = config_rpc::load_config_with_timeout().await?;

    // The existence check is the driver's now: `run_source_sync` resolves the id
    // against the registry it already reads for per-source budgets, and answers
    // `NotFound` for an id nobody registered. Looking it up here as well would
    // be a second read of the same file that can disagree with the one the
    // pipeline actually applies.
    let binding = crate::memory::binding::for_config(&config)?;
    // The enabled gate is not the driver's. `run_source_sync` runs whatever id
    // it is handed; it is `sources::sync::sync_source` and the periodic loop
    // that refuse a disabled entry, and this RPC is the third caller, the one
    // behind the user's Sync button. Same words as `sync_source` so the UI
    // reads one message (#5820).
    // Kept past the `if let` because the failure path below needs the host's own
    // answer for this id: a driver `NotFound` means something different when the
    // host has the source than when it does not.
    let host_entry = registry::get_source_in(&config, &req.source_id)?;
    if let Some(entry) = &host_entry {
        if !entry.enabled {
            return Err(format!("source '{}' is disabled", entry.id));
        }
        // Composio rows never reach the memory driver's pipeline — see
        // [`SyncDispatch`], which both this button and the Apply-all sweep read
        // the rule from.
        if let SyncDispatch::Connector {
            connection_id,
            max_items,
        } = sync_dispatch(entry)?
        {
            crate::integrations::composio::ops::composio_sync_budgeted(
                &config,
                &connection_id,
                Some("manual".to_string()),
                Some(entry.id.clone()),
                max_items,
            )
            .await?;
            return Ok(RpcOutcome::new(
                SyncResponse {
                    requested: true,
                    source_id: req.source_id,
                },
                vec![],
            ));
        }
    }
    // Resolved after the composio branch on purpose: the connector-backed
    // dispatch above needs `as_sources`, not `as_source_sync`, and a driver
    // serving the former without the latter must not fail a composio sync on
    // a capability it never uses (review finding on #5932).
    let sync = binding.provider().as_source_sync().ok_or_else(|| {
        format!(
            "the bound memory driver '{}' does not serve source sync",
            binding.driver_id()
        )
    })?;
    sync.run_source_sync(&req.source_id)
        .await
        .map_err(|error| {
            describe_source_sync_failure(&req.source_id, host_entry.is_some(), &error)
        })?;

    Ok(RpcOutcome::new(
        SyncResponse {
            requested: true,
            source_id: req.source_id,
        },
        vec![],
    ))
}

// ── Reconcile ──

#[derive(Debug, Default, serde::Deserialize)]
pub struct ReconcileRequest {
    /// Restrict to one source; omit to inspect every enabled source.
    #[serde(default)]
    pub source_id: Option<String>,
    /// When true, kick off background summarise+ingest for every scope
    /// with pending files. When false (default), report-only.
    #[serde(default)]
    pub execute: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ReconcileScopeReport {
    pub source_id: String,
    pub tree_scope: String,
    /// Raw `.md` files on disk for this scope.
    pub total_raw_files: u64,
    /// Files already covered by a tree summary.
    pub covered: u64,
    /// Files awaiting summarisation into the tree.
    ///
    /// A count the driver computed, not one this handler derived from a list.
    /// The engine's coverage scan returned each pending file's absolute path
    /// inside the content vault and this handler called `.len()` on it;
    /// `RawArchiveCoverage` deliberately reports the count alone, because a
    /// path describes the driver's storage layout and nothing here ever read
    /// one. The number is the same number.
    pub pending: u64,
    /// True when `execute` was set and a background reconcile was started.
    pub started: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ReconcileResponse {
    pub scopes: Vec<ReconcileScopeReport>,
}

/// Report (and optionally repair) raw-archive → tree coverage for memory
/// sources. The same incremental reconcile runs automatically after every
/// sync; this RPC exposes it for inspection and manual triggering.
///
/// The scopes themselves are still derived host-side (`derive_scopes` reads the
/// registry row); what moved is the crosscheck and the repair, which are
/// `MemorySourceSync::raw_archive_coverage` and `rebuild_from_raw_archive`.
pub async fn reconcile_rpc(req: ReconcileRequest) -> Result<RpcOutcome<ReconcileResponse>, String> {
    use crate::memory::sources::sync::derive_scopes;

    tracing::info!(
        source_id = ?req.source_id,
        execute = req.execute,
        "[memory_sources] reconcile_rpc: entry"
    );

    let config = config_rpc::load_config_with_timeout().await?;
    let binding = crate::memory::binding::for_config(&config)?;
    let Some(sync) = binding.provider().as_source_sync() else {
        return Err(unserved(&binding, "source sync", "reconcile"));
    };

    let sources: Vec<MemorySourceEntry> = match &req.source_id {
        Some(id) => vec![registry::get_source(id)
            .await?
            .ok_or_else(|| format!("source '{id}' not found"))?],
        None => registry::list_sources().await?,
    };

    let mut reports: Vec<ReconcileScopeReport> = Vec::new();
    for source in sources.iter().filter(|s| s.enabled) {
        for scope in derive_scopes(source, &config) {
            let coverage = sync
                .raw_archive_coverage(&scope.tree_scope, &scope.archive_source_id)
                .await
                .map_err(|e| format!("coverage for {}: {e}", scope.tree_scope))?;
            let mut started = false;
            if req.execute && coverage.pending > 0 {
                // The binding, not the config: the repair is a driver call now,
                // and the spawned task must reach the same bound driver this
                // request resolved rather than re-deriving one.
                let binding = std::sync::Arc::clone(&binding);
                let tree_scope = scope.tree_scope.clone();
                let archive = scope.archive_source_id.clone();
                tokio::spawn(async move {
                    // Re-resolved inside the task because the borrow cannot
                    // cross the spawn. It answered `Some` a moment ago on this
                    // same binding, so `None` here would mean the driver
                    // changed underneath the request — logged loudly rather
                    // than returning as a silent no-op.
                    let Some(sync) = binding.provider().as_source_sync() else {
                        tracing::error!(
                            driver = %binding.driver_id(),
                            tree_scope = %tree_scope,
                            "[memory_sources] reconcile_rpc: background reconcile abandoned — \
                             driver stopped serving source sync between the report and the repair"
                        );
                        return;
                    };
                    match sync.rebuild_from_raw_archive(&tree_scope, &archive).await {
                        Ok(outcome) => tracing::info!(
                            tree_scope = %tree_scope,
                            files = outcome.files_read,
                            batches = outcome.batches,
                            "[memory_sources] reconcile_rpc: background reconcile complete"
                        ),
                        Err(e) => tracing::warn!(
                            tree_scope = %tree_scope,
                            error = %e,
                            "[memory_sources] reconcile_rpc: background reconcile failed"
                        ),
                    }
                });
                started = true;
            }
            tracing::debug!(
                driver = %binding.driver_id(),
                source_id = %source.id,
                tree_scope = %scope.tree_scope,
                total = coverage.total,
                covered = coverage.covered,
                pending = coverage.pending,
                started = started,
                "[memory_sources] reconcile_rpc: scope report"
            );
            reports.push(ReconcileScopeReport {
                source_id: source.id.clone(),
                tree_scope: scope.tree_scope,
                total_raw_files: coverage.total,
                covered: coverage.covered,
                pending: coverage.pending,
                started,
            });
        }
    }

    Ok(RpcOutcome::new(
        ReconcileResponse { scopes: reports },
        vec![],
    ))
}
