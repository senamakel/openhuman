//! RPC handler implementations for memory sources.
//!
//! # How this file reaches memory (#5560)
//!
//! Every memory call below goes through the bound driver — never through an
//! in-process engine handle. The four surfaces this file used to reach into
//! `tinymemory_core::tinycortex` for became contract members in tinymemory
//! v1.7.0, and the mapping is one-for-one:
//!
//! | What a handler needs | Contract member |
//! |---|---|
//! | coding-session discovery | `MemoryCodingSessions::coding_session_status` |
//! | coding-session ingestion | `MemoryCodingSessions::ingest_coding_sessions` |
//! | raw-archive coverage, and its repair | `MemorySourceSync::raw_archive_coverage` / `rebuild_from_raw_archive` |
//! | the sync audit log | `MemorySourceSync::sync_audit_log` |
//! | the sync price | `MemorySourceSync::estimate_sync_cost_usd` |
//!
//! Two of those look shortcuttable and are not, so the reasons are recorded
//! here rather than left to be re-derived:
//!
//! - **The audit log is not read through the `memory::sync::audit` host shim.**
//!   That path resolves, and taking it would move this file off the
//!   direct-engine-reference scanner while removing no coupling whatsoever —
//!   precisely the edit the ratchet's own docs call the one that would make the
//!   lint lie.
//! - **The price is asked, never computed here.** The same constants behind
//!   `estimate_sync_cost_usd` stamped `estimated_cost_usd` onto every audit row
//!   the driver has already written, and `monthly_cost_summary_rpc` below totals
//!   those very rows. A host-side copy of the arithmetic becomes a second price
//!   the moment either side is retuned, and one screen would then show a
//!   projection and a history priced differently with nothing to say which.
//!
//! ## Refusing when the driver does not serve the family
//!
//! `as_source_sync()` / `as_coding_sessions()` answering `None` means the bound
//! driver serves no such family, and every handler here **refuses, naming the
//! driver** (see [`unserved`]). None of them reports an empty log, a zero cost
//! or an empty source list instead.
//!
//! That is a deliberately different trade from the tree read handlers, which do
//! degrade to empty. There, "no hits" is a true statement about a driver that
//! keeps no summary tree. Here the family *is* the entire subject of the call,
//! so an empty success is indistinguishable from "nothing has ever synced" or
//! "no coding agent is installed" — a wrong answer the caller cannot tell from
//! a right one, on the two screens where the number is a promise about money
//! and about how long an import will take.
//!
//! ## On this file's length
//!
//! It is over the ~500-line guidance, and was before this change; it is not
//! split as part of it. The seam is visible, though, and the routing made it
//! sharper: `coding_session_status_rpc`, `ingest_coding_sessions_rpc`,
//! `reconcile_rpc`, `sync_audit_log_rpc`, `estimate_sync_cost_rpc` and
//! `monthly_cost_summary_rpc` are the driver-facing half — six handlers over
//! two capability families, all sharing the binding preamble — while the
//! registry CRUD between them talks only to `registry` and `readers` and never
//! resolves a binding at all.

use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::config::Config;
use crate::openhuman::memory::api::provider::sessions::{
    CodingSessionIngestReport, CodingSessionSource,
};
use crate::openhuman::memory::api::provider::sync::SyncAuditEntry;
use crate::openhuman::memory::binding::MemoryBinding;
use crate::openhuman::memory::sources::apply_kind_defaults;
use crate::openhuman::memory::sources::readers;
use crate::openhuman::memory::sources::registry::{self, MemorySourcePatch};
use crate::openhuman::memory::sources::types::{MemorySourceEntry, SourceKind};
use crate::rpc::RpcOutcome;

/// The coding-session ingest request, under this domain's own name.
///
/// Re-exported here so `schemas.rs` can name it as
/// `rpc::CodingSessionIngestRequest`, the way every other handler adapter in
/// that file names its request type. The path behind the alias is now the
/// contract's (`tinymemory_api::provider::sessions`) rather than the engine's,
/// which is what lets `ingest_coding_sessions_rpc` hand the value straight to
/// the driver with no conversion in between — the request that crosses the bus
/// is the request `schemas.rs` deserialised.
pub use crate::openhuman::memory::api::provider::sessions::CodingSessionIngestRequest;

/// The refusal a handler returns when the bound driver serves no such family.
///
/// One place, so the message and the log line cannot drift between the six
/// call sites, and so the driver id is always in both. See the module docs for
/// why these handlers refuse rather than degrade to an empty answer.
fn unserved(binding: &MemoryBinding, family: &str, call: &str) -> String {
    tracing::warn!(
        driver = %binding.driver_id(),
        family = %family,
        call = %call,
        "[memory_sources] refusing: bound driver does not serve this capability family"
    );
    format!(
        "the bound memory driver '{}' does not serve {family}",
        binding.driver_id()
    )
}

#[derive(Debug, serde::Serialize)]
pub struct CodingSessionStatusResponse {
    pub sources: Vec<CodingSessionSource>,
}

/// Discover what each supported coding agent's session store holds.
///
/// The scan happens driver-side and is bounded by the driver's own caps, which
/// is why this no longer needs a blocking worker: the walk that used to run on
/// this process's pool now runs behind the bus, and what comes back is counts.
/// `CodingSessionSource::scan_truncated` is how a caller learns the counts are
/// a floor — the same field the engine's `CodingSessionSourceStatus` carried,
/// under the same name.
pub async fn coding_session_status_rpc() -> Result<RpcOutcome<CodingSessionStatusResponse>, String>
{
    tracing::debug!("[memory_sources] coding_session_status_rpc: entry");
    let config = config_rpc::load_config_with_timeout().await?;
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
    let Some(sessions) = binding.provider().as_coding_sessions() else {
        return Err(unserved(
            &binding,
            "coding sessions",
            "coding_session_status",
        ));
    };

    let sources = sessions
        .coding_session_status()
        .await
        .map_err(|error| format!("coding session status: {error}"))?;

    tracing::debug!(
        driver = %binding.driver_id(),
        sources = sources.len(),
        files = sources
            .iter()
            .map(|source| source.session_files)
            .sum::<usize>(),
        truncated = sources.iter().any(|source| source.scan_truncated),
        "[memory_sources] coding_session_status_rpc: exit"
    );
    Ok(RpcOutcome::new(
        CodingSessionStatusResponse { sources },
        vec![],
    ))
}

/// Wall-clock ceiling for one `ingest_coding_sessions` RPC, sized to the number
/// of sessions the caller asked to backfill and hard-capped at the ceiling the
/// frontend can actually wait for.
///
/// The original formula (`120 + N*30`) assumed **one LLM call per session**.
/// That premise is false: TinyCortex's persona pipeline splits an oversized
/// session into windows (`WINDOW_CHARS`-sized chunks of evidence) and issues one
/// LLM call *per window*, so a multi-window session drives several sequential
/// calls. A dense backfill therefore blew the old budget — 15 sessions hit the
/// exact 570 s ceiling (`120 + 15*30`) and were killed mid-flight.
///
/// The per-session allowance is therefore sized for *multiple* windows, not one
/// call: `PER_SESSION_SECS` budgets ~3 sequential per-window LLM calls at the
/// windows' observed 20–45 s span (#5509). It is a deliberate flat estimate, not
/// a per-session window count.
///
/// The result is hard-capped at `HARD_CAP_SECS` for two reasons that are really
/// one. First, this is the true reachable ceiling: the frontend RPC client
/// clamps every per-call timeout to `PER_CALL_TIMEOUT_MAX_MS = 600 s`
/// (`app/src/services/coreRpcClient.ts`), so a server budget above that can never
/// be observed — the client aborts first. Second, that cap also bounds the
/// blocking-pool worker this budget guards: `max_sessions` is untrusted (an
/// advertised programmatic RPC, `platform/about_app/catalog_data.rs`), and
/// without the cap a caller passing 1000 would pin a thread for ~33 h. Capping
/// the resulting `Duration` — not the multiplier — makes both true at once.
///
/// Because a single pass is bounded, large histories drain across repeated passes
/// (client `drainCodingSessions`); the per-pass batch is sized so `BASE + N*PER`
/// stays under the cap for the UI's `CODING_SESSION_BATCH_MAX`, keeping the
/// server budget the *tighter* of the two so it returns a clean structured
/// timeout before the client's fetch aborts. This is a *ceiling to catch a wedged
/// run*, not a latency target.
///
/// `pub(crate)` so the module driver can size its own bus deadline from the
/// same formula (`modules::memory`). That call sits *inside* this one, and
/// tinybus gives every call a 30 s default deadline if nobody sets one — 19×
/// tighter than the smallest budget computed here, which is how a completed
/// import came to be reported as a failure (#5802). One formula, two layers.
pub(crate) fn ingest_budget(max_sessions: usize) -> std::time::Duration {
    /// Fixed overhead allowance (config load, discovery, process warm-up) added
    /// on top of the per-session budget.
    const BASE_SECS: u64 = 120;
    /// Per-session allowance, sized for ~3 sequential per-window LLM calls at the
    /// 20–45 s/window span observed in #5509 rather than the single call the old
    /// formula assumed.
    const PER_SESSION_SECS: u64 = 90;
    /// Hard ceiling on the whole budget. Mirrors the frontend's
    /// `PER_CALL_TIMEOUT_MAX_MS` (600 s) — a larger budget is unreachable because
    /// the client aborts first — and bounds the blocking worker against an
    /// untrusted `max_sessions`.
    const HARD_CAP_SECS: u64 = 600;

    let scaled = BASE_SECS.saturating_add((max_sessions as u64).saturating_mul(PER_SESSION_SECS));
    std::time::Duration::from_secs(scaled.min(HARD_CAP_SECS))
}

/// Distil local coding-agent transcripts into observations.
///
/// The pipeline runs driver-side now, which removes the blocking-worker hop
/// this handler used to need: the engine's persona pass carried borrowed path
/// state and was not `Send`, so it had to be driven from `spawn_blocking` with
/// a `block_on` inside. The contract member is an ordinary `Send` future, so
/// the RPC simply awaits it.
///
/// **The deadline stays here on purpose.** `MemoryCodingSessions` documents
/// that it cannot bound the wall-clock cost — each session is one or more
/// sequential model calls — and that a caller needing a deadline enforces it on
/// its own side. [`ingest_budget`] is that deadline, unchanged; a timeout is
/// still reported as a structured error rather than as a short report, because
/// a report the run never finished writing is not progress the caller can keep.
pub async fn ingest_coding_sessions_rpc(
    req: CodingSessionIngestRequest,
) -> Result<RpcOutcome<CodingSessionIngestReport>, String> {
    tracing::info!(
        backfill = req.backfill,
        max_sessions = req.max_sessions,
        "[memory_sources] ingest_coding_sessions_rpc: entry"
    );
    let config = Config::load_or_init()
        .await
        .map_err(|error| format!("load config for coding-session ingestion: {error}"))?;
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
    let Some(sessions) = binding.provider().as_coding_sessions() else {
        return Err(unserved(
            &binding,
            "coding sessions",
            "ingest_coding_sessions",
        ));
    };

    // Wall-clock ceiling so a stalled provider call or a wedged session step
    // can't keep the RPC waiting indefinitely (#4863 review), sized to the
    // requested backfill so a legitimate large run isn't killed mid-flight
    // while a genuine infinite hang still terminates. Read before `req` moves.
    let ingest_timeout = ingest_budget(req.max_sessions);
    let report = tokio::time::timeout(ingest_timeout, sessions.ingest_coding_sessions(req))
        .await
        .map_err(|_elapsed| {
            tracing::error!(
                driver = %binding.driver_id(),
                timeout_secs = ingest_timeout.as_secs(),
                "[memory_sources] ingest_coding_sessions_rpc: timed out"
            );
            format!(
                "ingest coding sessions: timed out after {}s",
                ingest_timeout.as_secs()
            )
        })?
        .map_err(|error| format!("ingest coding sessions: {error}"))?;

    tracing::info!(
        driver = %binding.driver_id(),
        mode = %report.mode,
        processed = report.sessions_processed,
        failed = report.sessions_failed,
        budget_hit = report.budget_hit,
        "[memory_sources] ingest_coding_sessions_rpc: exit"
    );
    Ok(RpcOutcome::new(report, vec![]))
}

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
    let active = crate::openhuman::memory::sources::reconcile::ensure_composio_sources().await;
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
fn filter_to_active_composio_sources(
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
    pub items: Vec<crate::openhuman::memory::sources::types::SourceItem>,
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
    pub content: crate::openhuman::memory::sources::types::SourceContent,
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

pub async fn sync_rpc(req: SyncRequest) -> Result<RpcOutcome<SyncResponse>, String> {
    tracing::info!(source_id = %req.source_id, "[memory_sources] sync_rpc: entry");

    let config = config_rpc::load_config_with_timeout().await?;

    // The existence check is the driver's now: `run_source_sync` resolves the id
    // against the registry it already reads for per-source budgets, and answers
    // `NotFound` for an id nobody registered. Looking it up here as well would
    // be a second read of the same file that can disagree with the one the
    // pipeline actually applies.
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
    let sync = binding.provider().as_source_sync().ok_or_else(|| {
        format!(
            "the bound memory driver '{}' does not serve source sync",
            binding.driver_id()
        )
    })?;
    // The enabled gate is not the driver's. `run_source_sync` runs whatever id
    // it is handed; it is `sources::sync::sync_source` and the periodic loop
    // that refuse a disabled entry, and this RPC is the third caller, the one
    // behind the user's Sync button. Same words as `sync_source` so the UI
    // reads one message (#5820).
    if let Some(entry) = super::registry::get_source_in(&config, &req.source_id)? {
        if !entry.enabled {
            return Err(format!("source '{}' is disabled", entry.id));
        }
    }
    sync.run_source_sync(&req.source_id)
        .await
        .map_err(|error| error.to_string())?;

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
    use crate::openhuman::memory::sources::sync::derive_scopes;

    tracing::info!(
        source_id = ?req.source_id,
        execute = req.execute,
        "[memory_sources] reconcile_rpc: entry"
    );

    let config = config_rpc::load_config_with_timeout().await?;
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
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

// ── Status List ──

#[derive(Debug, serde::Serialize)]
pub struct StatusListResponse {
    pub statuses: Vec<crate::openhuman::memory::sources::status::SourceStatus>,
}

pub async fn status_list_rpc() -> Result<RpcOutcome<StatusListResponse>, String> {
    tracing::debug!("[memory_sources] status_list_rpc: entry");
    let config = config_rpc::load_config_with_timeout().await?;
    let statuses = crate::openhuman::memory::sources::status::status_list(&config).await?;
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

/// Toolkit slugs the memory-sync layer can actually run, sourced from the
/// provider registry (`all_providers()`) — the single source of truth shared
/// with `scan_active_sync_targets`. Exposed so the Add Source picker can
/// disable connections whose toolkit has no provider instead of letting the
/// user add a dead source. See issue #3352.
pub async fn supported_toolkits_rpc() -> Result<RpcOutcome<SupportedToolkitsResponse>, String> {
    tracing::debug!("[memory_sources] supported_toolkits_rpc: entry");
    // Ensure the built-in providers are registered before we snapshot the
    // registry — in CLI / fresh-process contexts the startup hook that calls
    // this may not have run yet.
    crate::openhuman::memory::sync::composio::init_default_composio_sync_providers();

    let mut toolkits: Vec<String> =
        crate::openhuman::memory::sync::composio::all_composio_sync_providers()
            .iter()
            .map(|p| p.toolkit_slug().to_string())
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
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
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
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
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
fn summarise_month(entries: &[SyncAuditEntry], month: &str) -> MonthlyCostSummaryResponse {
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
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
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

// ── Apply All In ──

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
async fn trigger_enabled_syncs<F, Fut>(
    sources: &[MemorySourceEntry],
    mut trigger: F,
) -> (u32, Vec<String>)
where
    F: FnMut(String) -> Fut,
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
        match trigger(source.id.clone()).await {
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

    // Resolved once for the whole sweep: a driver that serves no sync has
    // nothing to say about any source, and refusing per source would turn one
    // fact into a warning per row.
    let binding = crate::openhuman::memory::binding::for_config(&config)?;
    let sync = binding.provider().as_source_sync().ok_or_else(|| {
        format!(
            "the bound memory driver '{}' does not serve source sync",
            binding.driver_id()
        )
    })?;

    let (sync_triggered, sync_errors) = trigger_enabled_syncs(&sources, |source_id| async move {
        sync.run_source_sync(&source_id)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
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

#[cfg(test)]
mod filter_tests {
    use super::*;
    use std::collections::HashSet;

    fn composio_entry(id: &str, connection_id: &str) -> MemorySourceEntry {
        MemorySourceEntry {
            id: id.to_string(),
            kind: SourceKind::Composio,
            label: format!("Gmail · {connection_id}"),
            enabled: true,
            toolkit: Some("gmail".to_string()),
            connection_id: Some(connection_id.to_string()),
            path: None,
            glob: None,
            url: None,
            branch: None,
            paths: Vec::new(),
            max_commits: None,
            max_issues: None,
            max_prs: None,
            query: None,
            since_days: None,
            max_items: Some(100),
            selector: None,
            max_tokens_per_sync: None,
            max_cost_per_sync_usd: None,
            sync_depth_days: Some(30),
        }
    }

    fn local_entry(id: &str) -> MemorySourceEntry {
        MemorySourceEntry {
            id: id.to_string(),
            kind: SourceKind::Folder,
            label: "Notes".to_string(),
            enabled: true,
            toolkit: None,
            connection_id: None,
            path: Some("/tmp/notes".to_string()),
            glob: None,
            url: None,
            branch: None,
            paths: Vec::new(),
            max_commits: None,
            max_issues: None,
            max_prs: None,
            query: None,
            since_days: None,
            max_items: None,
            selector: None,
            max_tokens_per_sync: None,
            max_cost_per_sync_usd: None,
            sync_depth_days: None,
        }
    }

    fn active_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// RC-A: an inactive connection's row is hidden, only the active one shows —
    /// but the input list is preserved (hide-not-delete; the filter never removes
    /// entries from `config.memory_sources`, it only subtracts from the view).
    #[test]
    fn hides_inactive_connection_keeps_active() {
        let sources = vec![
            composio_entry("src_a", "conn_A"), // inactive
            composio_entry("src_b", "conn_B"), // active
        ];
        let active = active_set(&["conn_B"]);
        let out = filter_to_active_composio_sources(sources.clone(), Some(&active));

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].connection_id.as_deref(), Some("conn_B"));
        // Both rows still exist in the original list — nothing was deleted.
        assert_eq!(sources.len(), 2);
    }

    /// RC-B: two rows with the same active connection_id collapse to one.
    #[test]
    fn dedupes_identical_connection_ids() {
        let sources = vec![
            composio_entry("src_1", "conn_B"),
            composio_entry("src_2", "conn_B"),
        ];
        let active = active_set(&["conn_B"]);
        let out = filter_to_active_composio_sources(sources, Some(&active));

        assert_eq!(out.len(), 1, "identical-id rows must collapse to one");
        assert_eq!(out[0].connection_id.as_deref(), Some("conn_B"));
    }

    /// A previously-inactive connection reappears (with its settings intact) once
    /// it is back in the active set — confirming hiding is transient, not removal.
    #[test]
    fn reactivated_connection_reappears_with_settings() {
        let sources = vec![composio_entry("src_a", "conn_A")];
        let active = active_set(&["conn_A"]);
        let out = filter_to_active_composio_sources(sources, Some(&active));

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].connection_id.as_deref(), Some("conn_A"));
        // Settings (caps) survive the round-trip — the row was never mutated.
        assert_eq!(out[0].max_items, Some(100));
        assert_eq!(out[0].sync_depth_days, Some(30));
    }

    /// Non-Composio sources have no connection and are always shown, regardless
    /// of the active set.
    #[test]
    fn non_composio_sources_always_shown() {
        let sources = vec![local_entry("src_local"), composio_entry("src_x", "conn_X")];
        // Active set excludes the composio connection entirely.
        let active = active_set(&[]);
        let out = filter_to_active_composio_sources(sources, Some(&active));

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, SourceKind::Folder);
    }

    /// Safety: when the scan was unavailable (`None`), the list passes through
    /// untouched — we must never hide every Composio source on a transient blip.
    #[test]
    fn none_active_set_shows_all() {
        let sources = vec![
            composio_entry("src_a", "conn_A"),
            composio_entry("src_b", "conn_B"),
            local_entry("src_local"),
        ];
        let out = filter_to_active_composio_sources(sources, None);
        assert_eq!(out.len(), 3, "failed scan must show all sources, hide none");
    }

    /// A Composio row missing its connection_id is kept rather than silently
    /// dropped, even when an active set is present.
    #[test]
    fn composio_without_connection_id_is_kept() {
        let mut orphan = composio_entry("src_orphan", "conn_unused");
        orphan.connection_id = None;
        let active = active_set(&["conn_B"]);
        let out = filter_to_active_composio_sources(vec![orphan], Some(&active));
        assert_eq!(out.len(), 1);
    }
}

#[cfg(test)]
mod supported_toolkits_tests {
    use super::*;

    /// The supported-toolkit set must include every built-in provider slug.
    /// Asserted via `contains` (not exact equality) because the provider
    /// registry is a process-global shared with other tests in this binary
    /// that may register ad-hoc dummy providers.
    #[tokio::test]
    async fn supported_toolkits_includes_builtin_providers() {
        let outcome = supported_toolkits_rpc()
            .await
            .expect("supported_toolkits_rpc should succeed");
        let toolkits = outcome.value.toolkits;

        for slug in ["clickup", "github", "gmail", "linear", "notion", "slack"] {
            assert!(
                toolkits.iter().any(|t| t == slug),
                "expected supported toolkits to include '{slug}', got {toolkits:?}"
            );
        }
    }

    /// The returned set must be sorted and free of duplicates.
    #[tokio::test]
    async fn supported_toolkits_is_sorted_and_deduped() {
        let outcome = supported_toolkits_rpc()
            .await
            .expect("supported_toolkits_rpc should succeed");
        let toolkits = outcome.value.toolkits;

        let mut sorted = toolkits.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            toolkits, sorted,
            "toolkits should be sorted and de-duplicated"
        );
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    /// The frontend's `CODING_SESSION_BATCH_MAX` (`app/src/services/memorySourcesService.ts`).
    /// Mirrored here so the cross-wire invariant below is checkable Rust-side; the
    /// two must move together.
    const CLIENT_BATCH_MAX: usize = 5;
    /// The frontend's `PER_CALL_TIMEOUT_MAX_MS` (`app/src/services/coreRpcClient.ts`),
    /// in seconds — the ceiling the client can actually wait for.
    const CLIENT_HARD_CAP_SECS: u64 = 600;
    /// The frontend's `CODING_SESSION_RPC_GRACE_MS`, in seconds.
    const CLIENT_GRACE_SECS: u64 = 15;

    /// The formula is `min(120 + N * 90, 600)` seconds.
    #[test]
    fn budget_scales_per_session_for_multiple_windows() {
        // Zero sessions → base overhead only.
        assert_eq!(ingest_budget(0).as_secs(), 120);
        // One session carries a full per-session (multi-window) allowance.
        assert_eq!(ingest_budget(1).as_secs(), 120 + 90);
        // The UI batch (5 sessions) gets 570s — sized so a pass fits under the
        // 600s reachable ceiling while a 15-session backlog drains across passes.
        assert_eq!(ingest_budget(CLIENT_BATCH_MAX).as_secs(), 120 + 5 * 90);
    }

    /// The budget is hard-capped at the reachable ceiling (600s), so an untrusted
    /// `max_sessions` cannot pin the blocking worker beyond it — and cannot
    /// overflow.
    #[test]
    fn budget_is_capped_at_the_reachable_ceiling() {
        assert_eq!(ingest_budget(1_000).as_secs(), CLIENT_HARD_CAP_SECS);
        // Anything at or above the cap yields exactly the ceiling, no overflow.
        assert_eq!(ingest_budget(usize::MAX).as_secs(), CLIENT_HARD_CAP_SECS);
        assert_eq!(ingest_budget(5_000).as_secs(), CLIENT_HARD_CAP_SECS);
    }

    /// The invariant #5509 actually needs, pinned across the wire: for the UI's
    /// batch size the server budget must (a) stay under the client's hard cap so
    /// the pass is reachable, and (b) be the *tighter* of the two — i.e. below the
    /// client's own timeout for the same batch — so the server returns a clean
    /// structured timeout before the client's fetch aborts. This is the guard
    /// that would have caught the server-only fix moving the ceiling from 570s to
    /// an unreachable 1920s.
    #[test]
    fn server_budget_is_reachable_and_tighter_than_the_client() {
        let server = ingest_budget(CLIENT_BATCH_MAX).as_secs();
        let client = 120 + (CLIENT_BATCH_MAX as u64) * 90 + CLIENT_GRACE_SECS;
        assert!(
            server <= CLIENT_HARD_CAP_SECS,
            "server budget {server}s must be reachable (<= {CLIENT_HARD_CAP_SECS}s client cap)"
        );
        assert!(
            client <= CLIENT_HARD_CAP_SECS,
            "client budget {client}s must stay under its own {CLIENT_HARD_CAP_SECS}s clamp"
        );
        assert!(
            server < client,
            "server budget {server}s must fire before the client's {client}s abort"
        );
    }
}

#[cfg(test)]
mod monthly_summary_tests {
    use super::*;
    use chrono::{DateTime, Utc};

    /// One audit row, dated `stamp`, costing `estimated` with no real charge.
    fn row(stamp: &str, items: u32, input: u64, output: u64, estimated: f64) -> SyncAuditEntry {
        SyncAuditEntry {
            timestamp: stamp
                .parse::<DateTime<Utc>>()
                .expect("an RFC 3339 timestamp"),
            source_id: "src_1".to_string(),
            source_kind: "composio".to_string(),
            scope: "gmail".to_string(),
            items_fetched: items,
            batches: 1,
            input_tokens: input,
            output_tokens: output,
            estimated_cost_usd: estimated,
            composio_actions_called: 0,
            composio_cost_usd: 0.0,
            actual_charged_usd: None,
            duration_ms: 10,
            success: true,
            error: None,
            tree_ingest_failures: 0,
            tree_error: None,
        }
    }

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < 1e-9
    }

    /// Only the requested month is totalled, and a row from an earlier month
    /// proves the read crossed the boundary.
    #[test]
    fn totals_the_month_and_reports_complete_when_the_read_reached_past_it() {
        let entries = vec![
            row("2026-08-20T10:00:00Z", 3, 100, 20, 0.5),
            row("2026-08-01T00:00:00Z", 2, 50, 10, 0.25),
            // Older month — excluded from the totals, and the proof the read
            // covered the whole of August.
            row("2026-07-31T23:59:59Z", 99, 9_999, 9_999, 42.0),
        ];
        let summary = summarise_month(&entries, "2026-08");

        assert_eq!(summary.month, "2026-08");
        assert_eq!(summary.total_syncs, 2);
        assert_eq!(summary.total_items, 5);
        assert_eq!(summary.total_input_tokens, 150);
        assert_eq!(summary.total_output_tokens, 30);
        assert!(close(summary.total_cost_usd, 0.75));
        assert!(
            summary.totals_complete,
            "a row older than the month proves every row inside it was read"
        );
    }

    /// The cap case: every row the driver returned is inside the month, so the
    /// totals may be a floor and must not claim to be complete.
    #[test]
    fn reports_incomplete_when_every_returned_row_is_inside_the_month() {
        let entries = vec![
            row("2026-08-20T10:00:00Z", 1, 10, 1, 0.1),
            row("2026-08-19T10:00:00Z", 1, 10, 1, 0.1),
        ];
        let summary = summarise_month(&entries, "2026-08");

        assert_eq!(summary.total_syncs, 2);
        assert!(
            !summary.totals_complete,
            "the read may have been cut off at the driver's cap — totals are a floor"
        );
    }

    /// An empty log has no rows the cap could have hidden, so zero is exact.
    #[test]
    fn an_empty_log_totals_zero_and_is_complete() {
        let summary = summarise_month(&[], "2026-08");

        assert_eq!(summary.total_syncs, 0);
        assert_eq!(summary.total_items, 0);
        assert!(close(summary.total_cost_usd, 0.0));
        assert!(summary.totals_complete);
    }

    /// The real charge wins over the estimate, and Composio's own action cost
    /// is added on top — the audit's own view of what a run cost, unchanged by
    /// the move onto the contract.
    #[test]
    fn cost_uses_the_actual_charge_and_adds_composio() {
        let mut charged = row("2026-08-20T10:00:00Z", 1, 10, 1, 0.10);
        charged.actual_charged_usd = Some(0.30);
        charged.composio_cost_usd = 0.05;
        let estimated_only = row("2026-08-21T10:00:00Z", 1, 10, 1, 0.20);

        let summary = summarise_month(&[charged, estimated_only], "2026-08");

        // 0.30 (actual) + 0.05 (composio) + 0.20 (estimate) — the estimate on
        // the charged row is not counted.
        assert!(close(summary.total_cost_usd, 0.55));
    }

    /// A row stamped in a later month (clock skew) is skipped rather than
    /// counted, and does not count as proof the read crossed the boundary —
    /// the same exclusion the engine-backed month filter made.
    #[test]
    fn a_future_stamped_row_is_skipped_and_proves_nothing() {
        let entries = vec![
            row("2026-09-01T00:00:00Z", 7, 700, 70, 9.0),
            row("2026-08-20T10:00:00Z", 1, 10, 1, 0.1),
        ];
        let summary = summarise_month(&entries, "2026-08");

        assert_eq!(summary.total_syncs, 1, "the September row is not August's");
        assert_eq!(summary.total_items, 1);
        assert!(
            !summary.totals_complete,
            "a newer row says nothing about how far back the read reached"
        );
    }
}

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod rpc_tests;
