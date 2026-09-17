//! CRUD over the source registry: list, get, add, update, remove, and the
//! per-source item browse (list items / read item). Talks only to
//! `registry` and `readers` and never resolves a memory-driver binding.

use crate::config::rpc as config_rpc;
use crate::memory::sources::apply_kind_defaults;
use crate::memory::sources::readers;
use crate::memory::sources::registry::{self, MemorySourcePatch};
use crate::memory::sources::types::{MemorySourceEntry, SourceKind};
use crate::rpc::RpcOutcome;

// ── List ──

#[derive(Debug, serde::Serialize)]
pub struct ListResponse {
    pub sources: Vec<MemorySourceEntry>,
}

pub async fn list_rpc() -> Result<RpcOutcome<ListResponse>, String> {
    tracing::debug!("[memory_sources] list_rpc: entry");
    // Lazily reconcile Composio connections into the registry so users
    // see freshly-connected integrations as memory sources immediately,
    // without waiting for a restart or for the connection_created hook
    // to fire (which only triggers on OAuth handoff, not on first launch
    // after the user previously connected something).
    //
    // The reconcile also hands back the live active-connection set it just
    // scanned, which we reuse to hide Composio rows whose connection is no
    // longer active (re-auth / token expiry leaves a stale row behind) and to
    // collapse identical same-id duplicates from any reconcile race. This is a
    // display-layer filter only — no row, setting, or ingested memory is
    // removed; an inactive connection's row simply reappears once it re-activates.
    let active = crate::memory::sources::reconcile::ensure_composio_sources().await;
    let sources = registry::list_sources().await?;
    let filtered = filter_to_active_composio_sources(sources, active.as_ref());
    tracing::debug!(
        active_known = active.is_some(),
        active = active.as_ref().map(|a| a.len()).unwrap_or(0),
        returned = filtered.len(),
        "[memory_sources] list_rpc: filtered listing to active connections"
    );
    Ok(RpcOutcome::new(ListResponse { sources: filtered }, vec![]))
}

/// Filter the registry listing down to the live, deduplicated set of sources.
///
/// Composio sources are kept only when their `connection_id` is in `active`
/// (the live active-connection set scanned by `ensure_composio_sources` this
/// poll), collapsed to one row per `connection_id` so a non-atomic
/// `upsert_composio_source` race can't surface identical duplicate rows.
/// Non-Composio sources (folder / git / …) have no connection and are always
/// shown.
///
/// `active == None` means the live scan was unavailable (config / network /
/// auth failure). We must NOT read that as "everything is inactive" and hide
/// every Composio source — so on `None` the list passes through untouched. This
/// is hide-not-delete: the worst case is a stale row showing briefly until the
/// next good scan, fully reversible. Pure (no I/O) so it is unit-tested directly.
///
/// `pub(super)` so `rpc`'s sibling test module can reach it through
/// `super::*` — see the parent module docs.
pub(super) fn filter_to_active_composio_sources(
    mut sources: Vec<MemorySourceEntry>,
    active: Option<&std::collections::HashSet<String>>,
) -> Vec<MemorySourceEntry> {
    let Some(active) = active else {
        // Scan unavailable — show everything rather than hiding all Composio rows.
        return sources;
    };
    let mut seen = std::collections::HashSet::new();
    sources.retain(|s| {
        if s.kind != SourceKind::Composio {
            return true; // no connection to reconcile against — always show
        }
        match s.connection_id.as_deref() {
            // Active connection, first occurrence of this id → keep.
            // Inactive (`!contains`) → hidden (RC-A); later duplicate of the
            // same id (`!seen.insert`) → collapsed (RC-B).
            Some(id) => active.contains(id) && seen.insert(id.to_string()),
            // Malformed Composio row with no connection_id — keep it visible
            // rather than silently dropping a user's source.
            None => true,
        }
    });
    sources
}

// ── Get ──

#[derive(Debug, serde::Deserialize)]
pub struct GetRequest {
    pub id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct GetResponse {
    pub source: Option<MemorySourceEntry>,
}

pub async fn get_rpc(req: GetRequest) -> Result<RpcOutcome<GetResponse>, String> {
    tracing::debug!(id = %req.id, "[memory_sources] get_rpc: entry");
    let source = registry::get_source(&req.id).await?;
    Ok(RpcOutcome::new(GetResponse { source }, vec![]))
}

// ── Add ──

#[derive(Debug, serde::Deserialize)]
pub struct AddRequest {
    pub kind: SourceKind,
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,

    // Kind-specific fields (flat)
    #[serde(default)]
    pub toolkit: Option<String>,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub max_commits: Option<u32>,
    #[serde(default)]
    pub max_issues: Option<u32>,
    #[serde(default)]
    pub max_prs: Option<u32>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub since_days: Option<u32>,
    #[serde(default)]
    pub max_items: Option<u32>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub max_tokens_per_sync: Option<u64>,
    #[serde(default)]
    pub max_cost_per_sync_usd: Option<f64>,
    #[serde(default)]
    pub sync_depth_days: Option<u32>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, serde::Serialize)]
pub struct AddResponse {
    pub source: MemorySourceEntry,
}

pub async fn add_rpc(req: AddRequest) -> Result<RpcOutcome<AddResponse>, String> {
    tracing::info!(
        kind = %req.kind.as_str(),
        label = %req.label,
        "[memory_sources] add_rpc: entry"
    );

    let mut entry = MemorySourceEntry {
        id: format!("src_{}", uuid::Uuid::new_v4().as_simple()),
        kind: req.kind,
        label: req.label,
        enabled: req.enabled,
        toolkit: req.toolkit,
        connection_id: req.connection_id,
        path: req.path,
        glob: req.glob,
        url: req.url,
        branch: req.branch,
        paths: req.paths,
        max_commits: req.max_commits,
        max_issues: req.max_issues,
        max_prs: req.max_prs,
        query: req.query,
        since_days: req.since_days,
        max_items: req.max_items,
        selector: req.selector,
        max_tokens_per_sync: req.max_tokens_per_sync,
        max_cost_per_sync_usd: req.max_cost_per_sync_usd,
        sync_depth_days: req.sync_depth_days,
    };

    // Apply conservative per-kind defaults when the caller left caps unset.
    apply_kind_defaults(&mut entry);

    let source = registry::add_source(entry).await?;
    Ok(RpcOutcome::new(AddResponse { source }, vec![]))
}

// ── Update ──

#[derive(Debug, serde::Deserialize)]
pub struct UpdateRequest {
    pub id: String,
    #[serde(flatten)]
    pub patch: MemorySourcePatch,
}

#[derive(Debug, serde::Serialize)]
pub struct UpdateResponse {
    pub source: MemorySourceEntry,
}

pub async fn update_rpc(req: UpdateRequest) -> Result<RpcOutcome<UpdateResponse>, String> {
    tracing::info!(id = %req.id, "[memory_sources] update_rpc: entry");
    let source = registry::update_source(&req.id, req.patch).await?;
    Ok(RpcOutcome::new(UpdateResponse { source }, vec![]))
}

// ── Remove ──

#[derive(Debug, serde::Deserialize)]
pub struct RemoveRequest {
    pub id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct RemoveResponse {
    pub removed: bool,
}

pub async fn remove_rpc(req: RemoveRequest) -> Result<RpcOutcome<RemoveResponse>, String> {
    tracing::info!(id = %req.id, "[memory_sources] remove_rpc: entry");
    let removed = registry::remove_source(&req.id).await?;
    Ok(RpcOutcome::new(RemoveResponse { removed }, vec![]))
}

// ── List Items ──

#[derive(Debug, serde::Deserialize)]
pub struct ListItemsRequest {
    pub source_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ListItemsResponse {
    pub items: Vec<crate::memory::sources::types::SourceItem>,
}

pub async fn list_items_rpc(
    req: ListItemsRequest,
) -> Result<RpcOutcome<ListItemsResponse>, String> {
    tracing::debug!(source_id = %req.source_id, "[memory_sources] list_items_rpc: entry");

    let source = registry::get_source(&req.source_id)
        .await?
        .ok_or_else(|| format!("source '{}' not found", req.source_id))?;

    let config = config_rpc::load_config_with_timeout().await?;
    let reader = readers::reader_for(&source.kind);
    let items = reader.list_items(&source, &config).await?;

    Ok(RpcOutcome::new(ListItemsResponse { items }, vec![]))
}

// ── Read Item ──

#[derive(Debug, serde::Deserialize)]
pub struct ReadItemRequest {
    pub source_id: String,
    pub item_id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ReadItemResponse {
    pub content: crate::memory::sources::types::SourceContent,
}

pub async fn read_item_rpc(req: ReadItemRequest) -> Result<RpcOutcome<ReadItemResponse>, String> {
    tracing::debug!(
        source_id = %req.source_id,
        item_id = %req.item_id,
        "[memory_sources] read_item_rpc: entry"
    );

    let source = registry::get_source(&req.source_id)
        .await?
        .ok_or_else(|| format!("source '{}' not found", req.source_id))?;

    let config = config_rpc::load_config_with_timeout().await?;
    let reader = readers::reader_for(&source.kind);
    let content = reader.read_item(&source, &req.item_id, &config).await?;

    Ok(RpcOutcome::new(ReadItemResponse { content }, vec![]))
}
