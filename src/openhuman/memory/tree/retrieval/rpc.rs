//! JSON-RPC handler bodies for Phase 4 retrieval tools (#710).
//!
//! Shapes mirror the contract's — `RetrievalResponse` and `Vec<RetrievalHit>` /
//! `Vec<EntityMatch>` all serialise directly, without an extra envelope.
//!
//! # All five read through `MemoryRetrieval`
//!
//! Member for member: `retrieve_source`, `cover_window`, `retrieve_children`,
//! `retrieve_leaves`, `search_entities`. Nothing here reaches the engine.
//!
//! The four hit-returning handlers were held back while the pinned artifact was
//! TinyMemory v1.3.0, whose `RetrievalHit` had no `tree_kind`. The field is
//! `skip_serializing_if = "Option::is_none"`, so routing them then would have
//! decoded every hit's kind as `None` and dropped the key from four public
//! responses. v1.4.0 (tinymemory#95) carries it, and `modules::registry` plus
//! `ARTIFACT_CAPABILITIES_PIN` both name that release, so the loss is no longer
//! reachable: the engine's own `RetrievalHit::tree_kind` is a **non-optional**
//! `TreeKind`, the driver builds the contract's hit by serialising the engine's,
//! and a `#[serde(rename_all = "snake_case")]` enum against the contract's open
//! `String` encodes identically. Every hit that reaches this file therefore
//! carries `Some(kind)` and re-emits the same key with the same value
//! (`source`, `topic`, `global`, `flavoured`). A `None` would take a driver that
//! never ran the engine at all, and for that one the contract's reading — no
//! tree, so no kind to report — is why the key is omitted rather than sent as
//! `null`; inventing `source` there would be the silent-wrong-data case the
//! placeholder rule already rejects for summaries.
//!
//! `EntityMatch` is the same kind of identity: the engine's `kind` is a
//! snake_case enum where the contract's is an open `String`.
//!
//! The envelopes match field for field and in declaration order —
//! `RetrievalResponse` against the engine's `QueryResponse` (`hits`, `total`,
//! `truncated`) and `RetrievalHit` against the engine's (`node_id` through
//! `source_ref`) — so the JSON these handlers emit is byte-identical to what
//! they emitted while they called the engine directly. The frontend reads it.
//!
//! # The scope argument is the source gate, and is never a literal `None`
//!
//! `binding.provider()` is the **unguarded** driver: nothing between this file
//! and the store re-applies the per-profile `memory_sources` allowlist, so the
//! scope each call passes is the gate. The engine's `*_scoped` entry points
//! existed for the same reason — their ambient twins read the ENGINE's
//! task-local, which a separately compiled module cannot see, and an absent
//! scope means unrestricted, i.e. the gate failing open.
//!
//! Every call below passes `source_scope::as_bus_scope()`, which renders this
//! host's own task-local in the contract's vocabulary. `None` from it means
//! genuinely unrestricted and must stay `None`: an **empty** `SourceScope`
//! denies every source-attributed row, so mapping "no restriction" onto one
//! would invert the policy. A literal `None` written at the call site instead
//! is the open failure this seam exists to prevent.
//!
//! `search_entities` is the one member that takes no scope on either side — the
//! entity index is not source-attributed — so there is nothing to pass.

use serde::{Deserialize, Serialize};

use crate::openhuman::config::Config;
// The contract's retrieval vocabulary, not the engine's: these handlers return
// what the driver handed back. The two encode identically (see the module
// docs), so this is a Rust-type change and not a wire one.
use crate::openhuman::memory::api::provider::retrieval::{
    CoverWindowQuery, EntityMatch, RetrievalHit, RetrievalResponse, SourceRetrievalQuery,
};
use crate::openhuman::memory::source_scope::as_bus_scope;
use crate::openhuman::memory::tree::score::extract::EntityKind;
use crate::rpc::RpcOutcome;
use tinymemory_api::chunks::SourceKind;

// ── query_source ──────────────────────────────────────────────────────

/// Request body for `memory_tree_query_source`. All fields are optional;
/// see [`MemoryRetrieval::retrieve_source`] for selection semantics.
///
/// [`MemoryRetrieval::retrieve_source`]: crate::openhuman::memory::api::provider::retrieval::MemoryRetrieval::retrieve_source
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QuerySourceRequest {
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub time_window_days: Option<u32>,
    /// Phase 4 (#710) — optional natural-language query string. When
    /// provided, candidates are reranked by cosine similarity to the
    /// query's embedding rather than sorted by recency. Legacy rows
    /// with no stored embedding fall to the bottom.
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// JSON-RPC handler body for `memory_tree_query_source`. Parses the request,
/// reads through the bound driver's `MemoryRetrieval` family, and wraps the
/// outcome with a PII-redacted log line.
pub async fn query_source_rpc(
    config: &Config,
    req: QuerySourceRequest,
) -> Result<RpcOutcome<RetrievalResponse>, String> {
    // Parsed before the driver is resolved, so an unknown kind stays a caller
    // error naming the offending value rather than a driver round trip that
    // matches nothing and reads as an empty store.
    let source_kind = match req.source_kind.as_deref() {
        Some(s) => Some(SourceKind::parse(s).map_err(|e| format!("query_source: {e}"))?),
        None => None,
    };
    let query = SourceRetrievalQuery {
        source_id: req.source_id.clone(),
        source_kind,
        time_window_days: req.time_window_days,
        query: req.query.clone(),
        // 0 is the engine's "no caller preference" sentinel, not a request for
        // zero rows, and the driver forwards `limit` verbatim — so the
        // absent-limit default stays the engine's, exactly as it was on the
        // direct call.
        limit: req.limit.unwrap_or(0),
    };
    // The explicit scope, never `None` — see the module docs.
    let scope = as_bus_scope();

    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let resp = match binding.provider().as_retrieval() {
        Some(retrieval) => retrieval
            .retrieve_source(&query, scope.as_ref())
            .await
            .map_err(|e| format!("query_source: {e}"))?,
        // Read-only, so an empty page is the honest answer: a driver with no
        // retrieval family keeps no summary tree to rank, which is a true
        // statement about it rather than a fault the caller can act on.
        None => {
            log::debug!(
                "[memory-tree][rpc] query_source: driver '{}' does not serve Retrieval; reporting empty",
                binding.driver_id()
            );
            RetrievalResponse::default()
        }
    };
    let n = resp.hits.len();
    // Omit scope / source_id from the log — can carry PII. Log counts only.
    Ok(RpcOutcome::single_log(
        resp,
        format!(
            "memory_tree: query_source has_source_id={} source_kind={:?} has_query={} hits={}",
            req.source_id.is_some(),
            req.source_kind,
            req.query.is_some(),
            n
        ),
    ))
}

// ── cover_window ──────────────────────────────────────────────────────

/// Request body for `memory_tree_cover_window`. `since_ms`/`until_ms` are the
/// inclusive window bounds in epoch-milliseconds; the source filter mirrors
/// `query_source`. See [`MemoryRetrieval::cover_window`] for cover semantics.
///
/// [`MemoryRetrieval::cover_window`]: crate::openhuman::memory::api::provider::retrieval::MemoryRetrieval::cover_window
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoverWindowRequest {
    pub since_ms: i64,
    pub until_ms: i64,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// JSON-RPC handler body for `memory_tree_cover_window`. Parses the request,
/// reads through the bound driver, logs PII-redacted counts.
///
/// An inverted window is the driver's rejection to make, not this handler's:
/// the bound engine already refuses one naming both bounds, and duplicating the
/// guard here would let the two disagree about what a valid window is.
pub async fn cover_window_rpc(
    config: &Config,
    req: CoverWindowRequest,
) -> Result<RpcOutcome<RetrievalResponse>, String> {
    log::debug!(
        "[rpc][memory_tree] cover_window enter since_ms={} until_ms={} has_source_id={} has_source_kind={} has_limit={}",
        req.since_ms,
        req.until_ms,
        req.source_id.is_some(),
        req.source_kind.is_some(),
        req.limit.is_some()
    );
    let source_kind = match req.source_kind.as_deref() {
        Some(s) => {
            log::trace!("[rpc][memory_tree] cover_window parse_source_kind");
            Some(SourceKind::parse(s).map_err(|e| format!("cover_window: {e}"))?)
        }
        None => None,
    };
    log::trace!(
        "[rpc][memory_tree] cover_window dispatch limit={:?}",
        req.limit
    );
    let window = CoverWindowQuery {
        since_ms: req.since_ms,
        until_ms: req.until_ms,
        source_id: req.source_id.clone(),
        source_kind,
        // Forwarded as the caller sent it: the driver maps an absent limit onto
        // the engine's 0 sentinel, which is what `None` has always meant here.
        limit: req.limit,
    };
    // The explicit scope, never `None` — see the module docs.
    let scope = as_bus_scope();

    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let resp = match binding.provider().as_retrieval() {
        Some(retrieval) => retrieval
            .cover_window(&window, scope.as_ref())
            .await
            .map_err(|e| format!("cover_window: {e}"))?,
        // Read-only: a driver with no retrieval family has no nodes to cover
        // the window with, which is a true statement about it.
        None => {
            log::debug!(
                "[memory-tree][rpc] cover_window: driver '{}' does not serve Retrieval; reporting empty",
                binding.driver_id()
            );
            RetrievalResponse::default()
        }
    };
    let n = resp.hits.len();
    log::debug!(
        "[rpc][memory_tree] cover_window exit hits={} total={}",
        n,
        resp.total
    );
    // Omit scope / source_id from the log — can carry PII. Counts only.
    Ok(RpcOutcome::single_log(
        resp,
        format!(
            "memory_tree: cover_window since_ms={} until_ms={} has_source_id={} source_kind={:?} hits={}",
            req.since_ms,
            req.until_ms,
            req.source_id.is_some(),
            req.source_kind,
            n
        ),
    ))
}

// ── search_entities ───────────────────────────────────────────────────

/// Request body for `memory_tree_search_entities`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchEntitiesRequest {
    pub query: String,
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Response envelope for `memory_tree_search_entities`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchEntitiesResponse {
    pub matches: Vec<EntityMatch>,
}

/// JSON-RPC handler body for `memory_tree_search_entities`. Validates the
/// optional `kinds` filter against [`EntityKind`], then reads the entity index
/// through the bound driver's `MemoryRetrieval` family.
pub async fn search_entities_rpc(
    config: &Config,
    req: SearchEntitiesRequest,
) -> Result<RpcOutcome<SearchEntitiesResponse>, String> {
    // Capture logging-friendly summary BEFORE we move fields out of `req`.
    let query_len = req.query.len();
    let has_kinds = req.kinds.is_some();
    // Parsed before the driver is resolved, so an unknown kind stays a caller
    // error naming the offending value rather than a driver round trip that
    // matches nothing and reads as an empty index — the ambiguity the contract
    // requires this filter to be validated against. `EntityKind::parse` is
    // exact-match, so `as_str` re-emits what a caller that got it right sent.
    let kinds: Option<Vec<String>> = match req.kinds {
        None => None,
        Some(list) => {
            let parsed: Result<Vec<String>, String> = list
                .iter()
                .map(|s| {
                    EntityKind::parse(s)
                        .map(|kind| kind.as_str().to_string())
                        .map_err(|e| format!("search_entities: {e}"))
                })
                .collect();
            Some(parsed?)
        }
    };
    // 0 is the engine's "no caller preference" sentinel, not a request for zero
    // rows, and the driver forwards `limit` verbatim — so the absent-limit
    // default stays the engine's, exactly as it was on the direct call.
    let limit = req.limit.unwrap_or(0);

    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let matches = match binding.provider().as_retrieval() {
        Some(retrieval) => retrieval
            .search_entities(&req.query, kinds.as_deref(), limit)
            .await
            .map_err(|e| format!("search_entities: {e}"))?,
        // Read-only, so an empty match list is the honest answer: a driver with
        // no retrieval family keeps no entity index to search, which is a true
        // statement about it rather than a fault the caller can act on.
        None => {
            log::debug!(
                "[memory-tree][rpc] search_entities: driver '{}' does not serve Retrieval; reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };
    let n = matches.len();
    // Don't log the raw search query — can be an email, handle, etc. Log
    // only its length and the kind filter.
    Ok(RpcOutcome::single_log(
        SearchEntitiesResponse { matches },
        format!("memory_tree: search_entities query_len={query_len} has_kinds={has_kinds} n={n}"),
    ))
}

// ── drill_down ────────────────────────────────────────────────────────

/// Request body for `memory_tree_drill_down`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrillDownRequest {
    pub node_id: String,
    #[serde(default)]
    pub max_depth: Option<u32>,
    /// When set, visited children are reranked by cosine similarity between
    /// the query embedding and each child's stored embedding. Legacy children
    /// without an embedding sort to the bottom.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional cap on the returned hit count, applied AFTER rerank so the
    /// top-K is relevance-based when `query` is provided.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Response envelope for `memory_tree_drill_down`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrillDownResponse {
    pub hits: Vec<RetrievalHit>,
}

/// JSON-RPC handler body for `memory_tree_drill_down`.
///
/// Reads through the contract's `retrieve_children`, which is this tool's
/// member despite the name: the tree family's `drill_down` returns a node and
/// its direct children, where this returns ranked hits several levels deep.
pub async fn drill_down_rpc(
    config: &Config,
    req: DrillDownRequest,
) -> Result<RpcOutcome<DrillDownResponse>, String> {
    let depth = req.max_depth.unwrap_or(1);
    // The explicit scope, never `None` — see the module docs.
    let scope = as_bus_scope();

    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let hits = match binding.provider().as_retrieval() {
        Some(retrieval) => retrieval
            .retrieve_children(
                &req.node_id,
                depth,
                req.query.as_deref(),
                req.limit,
                scope.as_ref(),
            )
            .await
            .map_err(|e| format!("drill_down: {e}"))?,
        // Read-only, and an empty vector is already this handler's answer for a
        // node with no children — a driver with no retrieval family has no tree
        // to walk, so the degrade is indistinguishable from the ordinary miss.
        None => {
            log::debug!(
                "[memory-tree][rpc] drill_down: driver '{}' does not serve Retrieval; reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };
    let n = hits.len();
    // node_id can embed source scope (e.g. "chat:slack:#eng:0") which may
    // carry workspace hints — log only the structural prefix.
    let node_kind_prefix = req
        .node_id
        .split_once(':')
        .map(|(k, _)| k)
        .unwrap_or("unknown");
    Ok(RpcOutcome::single_log(
        DrillDownResponse { hits },
        format!(
            "memory_tree: drill_down node_kind={} depth={} has_query={} limit={:?} n={}",
            node_kind_prefix,
            depth,
            req.query.is_some(),
            req.limit,
            n
        ),
    ))
}

// ── fetch_leaves ──────────────────────────────────────────────────────

/// Request body for `memory_tree_fetch_leaves`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchLeavesRequest {
    pub chunk_ids: Vec<String>,
}

/// Response envelope for `memory_tree_fetch_leaves`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchLeavesResponse {
    pub hits: Vec<RetrievalHit>,
}

/// JSON-RPC handler body for `memory_tree_fetch_leaves`.
///
/// Ids that do not resolve are omitted by the driver, so the response may be
/// shorter than the request and callers must not index by position. A chunk
/// whose source falls outside the scope is omitted the same way, which is what
/// stops naming a chunk id directly from reading around the source gate.
pub async fn fetch_leaves_rpc(
    config: &Config,
    req: FetchLeavesRequest,
) -> Result<RpcOutcome<FetchLeavesResponse>, String> {
    // The explicit scope, never `None` — see the module docs. It matters most
    // here: this member takes ids the caller chose.
    let scope = as_bus_scope();

    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let hits = match binding.provider().as_retrieval() {
        Some(retrieval) => retrieval
            .retrieve_leaves(&req.chunk_ids, scope.as_ref())
            .await
            .map_err(|e| format!("fetch_leaves: {e}"))?,
        // Read-only, and omitting unresolvable ids is already this member's
        // contract — a driver with no retrieval family resolves none of them.
        None => {
            log::debug!(
                "[memory-tree][rpc] fetch_leaves: driver '{}' does not serve Retrieval; reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };
    let n = hits.len();
    Ok(RpcOutcome::single_log(
        FetchLeavesResponse { hits },
        format!("memory_tree: fetch_leaves n={n}"),
    ))
}

#[cfg(test)]
mod tests {
    //! Unit tests for the Phase 4 retrieval RPC handlers.
    //!
    //! Scope: the handler layer specifically — param parsing, default
    //! fallbacks, `SourceKind` / `EntityKind` validation, the scope each call
    //! forwards to the contract, `RpcOutcome` envelope shape, and PII-redacted
    //! log formatting. Retrieval correctness is the driver's and is covered by
    //! the engine's own tests; these deliberately do NOT re-verify it.
    //!
    //! All five handlers read through the bound driver, and a unit test cannot
    //! load the compiled module — so every test that gets past parameter
    //! validation binds one. [`bind_without_retrieval`] pins the degrade path;
    //! [`bind_recording`] pins what the handler hands the contract and what it
    //! does with the answer.
    //!
    //! One test binds the real in-process driver instead
    //! ([`install_tinycortex_for_test`]): the source gate has to be proved end
    //! to end, because "the handler passed a scope" and "a restricted profile
    //! cannot read another source" are different claims and only the second one
    //! is the security property. It is the driver the loadable module wraps,
    //! which is as close to production as a test process can get.
    //!
    //! [`install_tinycortex_for_test`]: crate::openhuman::memory::test_support::install_tinycortex_for_test
    use std::sync::{Arc, Mutex};

    use super::*;
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    use crate::openhuman::memory::api::capabilities::Capabilities;
    use crate::openhuman::memory::api::error::MemoryError;
    use crate::openhuman::memory::api::health::MemoryHealth;
    use crate::openhuman::memory::api::provider::retrieval::{
        FastRetrieveQuery, MemoryRetrieval, RetrievalNodeKind,
    };
    use crate::openhuman::memory::api::provider::types::{
        ExportPage, ExportRecord, ImportOutcome, SourceScope,
    };
    use crate::openhuman::memory::api::provider::{
        MemoryCore, MemoryPortability, MemoryProvider, MemoryRecall,
    };
    use crate::openhuman::memory::api::recall::OwnedRecallOpts;
    use crate::openhuman::memory::api::types::{
        MemoryCategory, MemoryEntry, MemoryTaint, NamespaceMemoryHit, NamespaceSummary,
    };
    use crate::openhuman::memory::source_scope::with_source_scope;
    // The engine-backed chunk writes these fixtures need live in
    // `retrieval::test_support` rather than here. They are the engine's chunk
    // store — `MemoryChunks` is read-only on the contract — and they are
    // test-only, which is exactly the pair `direct_engine_refs_tests`'
    // line-based scanner cannot tell apart when the reference sits inside an
    // inline `#[cfg(test)]` module. See that module's docs.
    use crate::openhuman::memory::tree::retrieval::test_support::{
        stage_test_chunks, upsert_chunks,
    };
    use tinymemory_api::chunks::{chunk_id, Chunk, Metadata, SourceRef};
    use tinymemory_api::null::NullMemoryProvider;

    fn test_config() -> (TempDir, Config) {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.workspace_dir = tmp.path().to_path_buf();
        // Inert embedder: the driver-backed test below must not reach a real
        // embedding endpoint, and none of these reads rank against a query.
        cfg.memory_tree.embedding_endpoint = None;
        cfg.memory_tree.embedding_model = None;
        cfg.memory_tree.embedding_strict = false;
        (tmp, cfg)
    }

    fn sample_chunk(source: &str, seq: u32) -> Chunk {
        let ts = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        Chunk {
            id: chunk_id(SourceKind::Chat, source, seq, "test-content"),
            content: format!("content-{source}-{seq}"),
            metadata: Metadata {
                source_kind: SourceKind::Chat,
                source_id: source.into(),
                owner: "alice".into(),
                timestamp: ts,
                time_range: (ts, ts),
                tags: vec![],
                source_ref: Some(SourceRef::new(format!("slack://{source}/{seq}"))),
                path_scope: None,
            },
            token_count: 20,
            seq_in_source: seq,
            created_at: ts,
            partial_message: false,
        }
    }

    /// Bind a driver with no retrieval family as `cfg`'s memory driver.
    ///
    /// `FixedDiagnostics` is `NullMemoryProvider`-backed and overrides only
    /// `as_maintenance`, so `as_retrieval()` is `None` — the shape of a driver
    /// that serves memory without exposing the engine's retrieval primitives.
    /// Every test that reaches a handler past its parameter validation needs a
    /// binding installed: without one, resolving a driver tries to load the
    /// compiled module, which in a test process can block rather than fail.
    fn bind_without_retrieval(cfg: &Config) {
        crate::openhuman::memory::binding::install_diagnostics_for_test(
            &cfg.workspace_dir,
            &cfg.subsystems.memory,
            Default::default(),
            Default::default(),
        );
    }

    /// Bind `driver` as `cfg`'s memory driver and keep a handle on it.
    fn bind_recording(cfg: &Config, driver: RecordingRetrieval) -> Arc<RecordingRetrieval> {
        let driver = Arc::new(driver);
        crate::openhuman::memory::binding::install_for_test(
            &cfg.workspace_dir,
            &cfg.subsystems.memory,
            Arc::clone(&driver) as Arc<dyn MemoryProvider>,
        );
        driver
    }

    /// One scripted hit, carrying the `tree_kind` every engine-produced hit has.
    fn hit(node_id: &str) -> RetrievalHit {
        let ts = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        RetrievalHit {
            node_id: node_id.to_string(),
            node_kind: RetrievalNodeKind::Leaf,
            tree_id: String::new(),
            tree_kind: Some("source".to_string()),
            tree_scope: String::new(),
            level: 0,
            content: format!("content-{node_id}"),
            entities: Vec::new(),
            topics: Vec::new(),
            time_range_start: ts,
            time_range_end: ts,
            score: 1.0,
            child_ids: Vec::new(),
            source_ref: None,
        }
    }

    /// What one contract call was handed.
    #[derive(Default)]
    struct Calls {
        /// The scope argument per member, in call order. Recorded because an
        /// absent scope means UNRESTRICTED on the far side: a handler that
        /// passed `None` where a turn had an allowlist would fail the source
        /// gate open, and the answer would look identical either way.
        scopes: Vec<(String, Option<SourceScope>)>,
        source_queries: Vec<SourceRetrievalQuery>,
        windows: Vec<CoverWindowQuery>,
    }

    /// A driver whose only advertised behaviour is retrieval: it records the
    /// arguments it was handed and answers with scripted hits, or with a
    /// rejection when one is scripted.
    struct RecordingRetrieval {
        inner: NullMemoryProvider,
        hits: Vec<RetrievalHit>,
        invalid: Option<String>,
        calls: Mutex<Calls>,
    }

    impl RecordingRetrieval {
        fn new() -> Self {
            Self {
                inner: NullMemoryProvider::new(),
                hits: Vec::new(),
                invalid: None,
                calls: Mutex::new(Calls::default()),
            }
        }

        fn answering(mut self, hits: Vec<RetrievalHit>) -> Self {
            self.hits = hits;
            self
        }

        /// Reject every retrieval call with `message`, the way the engine
        /// rejects an inverted window.
        fn rejecting(mut self, message: &str) -> Self {
            self.invalid = Some(message.to_string());
            self
        }

        fn record(&self, member: &str, scope: Option<&SourceScope>) {
            self.calls
                .lock()
                .expect("calls lock")
                .scopes
                .push((member.to_string(), scope.cloned()));
        }

        /// The scope `member` was handed. The outer `Option` distinguishes
        /// "never called" from "called with no scope".
        fn scope_for(&self, member: &str) -> Option<Option<SourceScope>> {
            self.calls
                .lock()
                .expect("calls lock")
                .scopes
                .iter()
                .find(|(m, _)| m == member)
                .map(|(_, scope)| scope.clone())
        }

        fn source_query(&self) -> SourceRetrievalQuery {
            self.calls
                .lock()
                .expect("calls lock")
                .source_queries
                .first()
                .cloned()
                .expect("retrieve_source was called")
        }

        fn window(&self) -> CoverWindowQuery {
            self.calls
                .lock()
                .expect("calls lock")
                .windows
                .first()
                .cloned()
                .expect("cover_window was called")
        }

        fn answer(&self) -> Result<Vec<RetrievalHit>, MemoryError> {
            match &self.invalid {
                Some(message) => Err(MemoryError::Invalid(message.clone())),
                None => Ok(self.hits.clone()),
            }
        }

        fn page(&self) -> Result<RetrievalResponse, MemoryError> {
            let hits = self.answer()?;
            let total = hits.len();
            Ok(RetrievalResponse {
                hits,
                total,
                truncated: false,
            })
        }
    }

    #[async_trait]
    impl MemoryRetrieval for RecordingRetrieval {
        async fn cover_window(
            &self,
            window: &CoverWindowQuery,
            scope: Option<&SourceScope>,
        ) -> Result<RetrievalResponse, MemoryError> {
            self.record("cover_window", scope);
            self.calls
                .lock()
                .expect("calls lock")
                .windows
                .push(window.clone());
            self.page()
        }

        async fn retrieve_source(
            &self,
            query: &SourceRetrievalQuery,
            scope: Option<&SourceScope>,
        ) -> Result<RetrievalResponse, MemoryError> {
            self.record("retrieve_source", scope);
            self.calls
                .lock()
                .expect("calls lock")
                .source_queries
                .push(query.clone());
            self.page()
        }

        async fn retrieve_children(
            &self,
            _node_id: &str,
            _max_depth: u32,
            _query: Option<&str>,
            _limit: Option<usize>,
            scope: Option<&SourceScope>,
        ) -> Result<Vec<RetrievalHit>, MemoryError> {
            self.record("retrieve_children", scope);
            self.answer()
        }

        async fn retrieve_leaves(
            &self,
            _chunk_ids: &[String],
            scope: Option<&SourceScope>,
        ) -> Result<Vec<RetrievalHit>, MemoryError> {
            self.record("retrieve_leaves", scope);
            self.answer()
        }

        // The family's remaining members are not reachable from these handlers.
        // They say so rather than returning a plausible empty value that could
        // make a future test pass for the wrong reason.
        async fn fast_retrieve(
            &self,
            _query: &str,
            _options: FastRetrieveQuery,
            _scope: Option<&SourceScope>,
        ) -> Result<RetrievalResponse, MemoryError> {
            unimplemented!("no retrieval RPC reaches fast_retrieve")
        }

        async fn recall_namespace_scored(
            &self,
            _namespace: &str,
            _query: &str,
            _limit: usize,
            _exclude_session_id: Option<&str>,
        ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
            unimplemented!("no retrieval RPC reaches recall_namespace_scored")
        }

        async fn recall_namespace_recent(
            &self,
            _namespace: &str,
            _limit: usize,
        ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
            unimplemented!("no retrieval RPC reaches recall_namespace_recent")
        }

        async fn search_entities(
            &self,
            _query: &str,
            _kinds: Option<&[String]>,
            _limit: usize,
        ) -> Result<Vec<EntityMatch>, MemoryError> {
            unimplemented!("search_entities has its own degrade-path tests")
        }
    }

    // The mandatory three are supertraits of `MemoryProvider`, so a stub cannot
    // skip them. Delegated to the null driver: this double exists to observe
    // retrieval, and nothing here stores or recalls.
    #[async_trait]
    impl MemoryCore for RecordingRetrieval {
        async fn store(
            &self,
            namespace: &str,
            key: &str,
            content: &str,
            category: MemoryCategory,
            session_id: Option<&str>,
            taint: MemoryTaint,
        ) -> Result<(), MemoryError> {
            self.inner
                .store(namespace, key, content, category, session_id, taint)
                .await
        }

        async fn get(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<MemoryEntry>, MemoryError> {
            self.inner.get(namespace, key).await
        }

        async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
            self.inner.forget(namespace, key).await
        }

        async fn list(
            &self,
            namespace: Option<&str>,
            category: Option<&MemoryCategory>,
            session_id: Option<&str>,
        ) -> Result<Vec<MemoryEntry>, MemoryError> {
            self.inner.list(namespace, category, session_id).await
        }

        async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
            self.inner.namespaces().await
        }
    }

    #[async_trait]
    impl MemoryRecall for RecordingRetrieval {
        async fn recall(
            &self,
            query: &str,
            limit: usize,
            opts: &OwnedRecallOpts,
            scope: Option<&SourceScope>,
        ) -> Result<Vec<MemoryEntry>, MemoryError> {
            self.inner.recall(query, limit, opts, scope).await
        }
    }

    #[async_trait]
    impl MemoryPortability for RecordingRetrieval {
        async fn export_page(
            &self,
            cursor: Option<&str>,
            limit: usize,
        ) -> Result<ExportPage, MemoryError> {
            self.inner.export_page(cursor, limit).await
        }

        async fn import_records(
            &self,
            records: Vec<ExportRecord>,
        ) -> Result<ImportOutcome, MemoryError> {
            self.inner.import_records(records).await
        }
    }

    #[async_trait]
    impl MemoryProvider for RecordingRetrieval {
        fn driver_id(&self) -> &str {
            "recording-retrieval"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::all()
        }

        async fn health(&self) -> MemoryHealth {
            MemoryHealth::Ready
        }

        fn as_retrieval(&self) -> Option<&dyn MemoryRetrieval> {
            Some(self)
        }
    }

    // ── query_source_rpc ──────────────────────────────────────────────

    /// An unknown kind is rejected **before** a driver is resolved, which is why
    /// this test installs no binding: a caller mistake must not need a working
    /// driver to be reported, and must not reach one and come back as an empty
    /// store instead.
    #[tokio::test]
    async fn query_source_rpc_rejects_invalid_source_kind() {
        let (_tmp, cfg) = test_config();
        let req = QuerySourceRequest {
            source_id: None,
            source_kind: Some("bogus".into()),
            time_window_days: None,
            query: None,
            limit: None,
        };
        let err = query_source_rpc(&cfg, req).await.unwrap_err();
        assert!(err.contains("unknown source kind: bogus"), "got {err}");
    }

    /// The read degrades rather than fails when the bound driver has no
    /// retrieval family, and still logs the count it served — a silent empty
    /// and a degraded empty look identical downstream otherwise.
    #[tokio::test]
    async fn query_source_rpc_degrades_to_empty_without_the_retrieval_family() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let outcome = query_source_rpc(&cfg, QuerySourceRequest::default())
            .await
            .expect("a driver without the retrieval family is not an error");
        assert!(outcome.value.hits.is_empty());
        assert_eq!(outcome.value.total, 0);
        assert_eq!(outcome.logs.len(), 1);
        let log = &outcome.logs[0];
        assert!(log.contains("has_source_id=false"), "log: {log}");
        assert!(log.contains("source_kind=None"), "log: {log}");
        assert!(log.contains("has_query=false"), "log: {log}");
        assert!(log.contains("hits=0"), "log: {log}");
    }

    #[tokio::test]
    async fn query_source_rpc_redacts_source_id_from_its_log() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = QuerySourceRequest {
            source_id: Some("slack:#eng".into()),
            source_kind: Some("chat".into()),
            time_window_days: None,
            query: None,
            limit: Some(5),
        };
        let outcome = query_source_rpc(&cfg, req).await.unwrap();
        assert!(outcome.value.hits.is_empty());
        let log = &outcome.logs[0];
        assert!(log.contains("has_source_id=true"), "log: {log}");
        assert!(log.contains("source_kind=Some(\"chat\")"), "log: {log}");
        // PII redaction: the raw source_id must NOT leak into the log.
        assert!(!log.contains("slack:#eng"), "log leaked source_id: {log}");
    }

    /// The filters reach the contract intact, and so does the turn's source
    /// allowlist — the gate this handler is the only thing applying.
    #[tokio::test]
    async fn query_source_rpc_forwards_its_filters_and_the_turn_scope() {
        let (_tmp, cfg) = test_config();
        let driver = bind_recording(&cfg, RecordingRetrieval::new());
        let req = QuerySourceRequest {
            source_id: Some("slack:#eng".into()),
            source_kind: Some("chat".into()),
            time_window_days: Some(7),
            query: Some("phoenix".into()),
            limit: Some(5),
        };
        with_source_scope(Some(vec!["slack:#eng".into()]), async {
            query_source_rpc(&cfg, req).await.unwrap()
        })
        .await;

        let query = driver.source_query();
        assert_eq!(query.source_id.as_deref(), Some("slack:#eng"));
        assert_eq!(query.source_kind, Some(SourceKind::Chat));
        assert_eq!(query.time_window_days, Some(7));
        assert_eq!(query.query.as_deref(), Some("phoenix"));
        assert_eq!(query.limit, 5);

        let scope = driver
            .scope_for("retrieve_source")
            .expect("retrieve_source was called")
            .expect("a restricted turn must not reach the driver as unrestricted");
        assert_eq!(scope.allow, vec!["slack:#eng".to_string()]);
    }

    /// Outside a turn there is genuinely no restriction, and that has to travel
    /// as `None`: an **empty** `SourceScope` denies every source-attributed row,
    /// so mapping "unrestricted" onto one would blank recall out instead.
    #[tokio::test]
    async fn query_source_rpc_leaves_the_scope_absent_when_unrestricted() {
        let (_tmp, cfg) = test_config();
        let driver = bind_recording(&cfg, RecordingRetrieval::new());
        query_source_rpc(&cfg, QuerySourceRequest::default())
            .await
            .unwrap();
        assert_eq!(
            driver.scope_for("retrieve_source"),
            Some(None),
            "no ambient allowlist must reach the driver as None, not Some(empty)"
        );
    }

    /// An absent `limit` stays the engine's default rather than becoming a
    /// request for zero rows.
    #[tokio::test]
    async fn query_source_rpc_maps_an_absent_limit_to_the_engine_sentinel() {
        let (_tmp, cfg) = test_config();
        let driver = bind_recording(&cfg, RecordingRetrieval::new());
        query_source_rpc(&cfg, QuerySourceRequest::default())
            .await
            .unwrap();
        assert_eq!(driver.source_query().limit, 0);
    }

    // ── cover_window_rpc ──────────────────────────────────────────────

    #[tokio::test]
    async fn cover_window_rpc_rejects_invalid_source_kind() {
        let (_tmp, cfg) = test_config();
        let req = CoverWindowRequest {
            since_ms: 0,
            until_ms: 1,
            source_id: None,
            source_kind: Some("bogus".into()),
            limit: None,
        };
        let err = cover_window_rpc(&cfg, req).await.unwrap_err();
        assert!(err.contains("cover_window:"), "got {err}");
        assert!(err.contains("unknown source kind: bogus"), "got {err}");
    }

    #[tokio::test]
    async fn cover_window_rpc_degrades_to_empty_and_redacts_its_log() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = CoverWindowRequest {
            since_ms: 0,
            until_ms: 4_000_000_000_000,
            source_id: Some("slack:#eng".into()),
            source_kind: Some("chat".into()),
            limit: None,
        };
        let outcome = cover_window_rpc(&cfg, req)
            .await
            .expect("a driver without the retrieval family is not an error");
        assert!(outcome.value.hits.is_empty());
        assert_eq!(outcome.value.total, 0);
        assert_eq!(outcome.logs.len(), 1);
        let log = &outcome.logs[0];
        assert!(log.contains("has_source_id=true"), "log: {log}");
        assert!(log.contains("source_kind=Some(\"chat\")"), "log: {log}");
        assert!(log.contains("hits=0"), "log: {log}");
        // PII redaction: the raw source_id must NOT leak into the log.
        assert!(!log.contains("slack:#eng"), "log leaked source_id: {log}");
    }

    #[tokio::test]
    async fn cover_window_rpc_forwards_its_window_and_the_turn_scope() {
        let (_tmp, cfg) = test_config();
        let driver = bind_recording(&cfg, RecordingRetrieval::new());
        let req = CoverWindowRequest {
            since_ms: 10,
            until_ms: 20,
            source_id: Some("slack:#eng".into()),
            source_kind: Some("chat".into()),
            limit: Some(3),
        };
        with_source_scope(Some(vec!["slack:#eng".into()]), async {
            cover_window_rpc(&cfg, req).await.unwrap()
        })
        .await;

        let window = driver.window();
        assert_eq!(window.since_ms, 10);
        assert_eq!(window.until_ms, 20);
        assert_eq!(window.source_id.as_deref(), Some("slack:#eng"));
        assert_eq!(window.source_kind, Some(SourceKind::Chat));
        // Forwarded as sent: the driver, not this handler, maps absence onto
        // the engine's 0 sentinel.
        assert_eq!(window.limit, Some(3));

        let scope = driver
            .scope_for("cover_window")
            .expect("cover_window was called")
            .expect("a restricted turn must not reach the driver as unrestricted");
        assert_eq!(scope.allow, vec!["slack:#eng".to_string()]);
    }

    /// An inverted window is now the driver's rejection, and it has to arrive as
    /// an error rather than as an empty page — the two are indistinguishable to
    /// a caller otherwise, and one of them is a bug report.
    #[tokio::test]
    async fn cover_window_rpc_surfaces_a_driver_rejection() {
        let (_tmp, cfg) = test_config();
        bind_recording(
            &cfg,
            RecordingRetrieval::new().rejecting("until_ms 50 is before since_ms 100"),
        );
        let req = CoverWindowRequest {
            since_ms: 100,
            until_ms: 50,
            source_id: None,
            source_kind: None,
            limit: None,
        };
        let err = cover_window_rpc(&cfg, req).await.unwrap_err();
        assert!(err.contains("cover_window:"), "got {err}");
        assert!(err.contains("until_ms"), "got {err}");
        assert!(err.contains("since_ms"), "got {err}");
    }

    /// The source gate, end to end through the real driver.
    ///
    /// The tests above prove the handler *passes* a scope; this one proves a
    /// restricted profile cannot read a source it was not granted. It has to
    /// bind the in-process driver rather than the double, because the filtering
    /// is the engine's — and `binding.provider()` is unguarded, so the scope
    /// this handler passes is the only thing standing between the two.
    #[tokio::test]
    async fn cover_window_rpc_honors_profile_source_scope() {
        let (_tmp, cfg) = test_config();
        // Two memory-source chunks in different sources, both inside the window.
        let mut allowed = sample_chunk("slack:#eng", 0);
        allowed.metadata.tags = vec!["memory_sources".into(), "chat".into()];
        let mut blocked = sample_chunk("slack:#secret", 0);
        blocked.metadata.tags = vec!["memory_sources".into(), "chat".into()];
        upsert_chunks(&cfg, &[allowed.clone(), blocked.clone()]).unwrap();
        stage_test_chunks(&cfg, &[allowed.clone(), blocked.clone()]);
        crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);

        let req = || CoverWindowRequest {
            since_ms: 0,
            until_ms: 4_000_000_000_000,
            source_id: None,
            source_kind: None,
            limit: None,
        };

        let resp = with_source_scope(Some(vec!["slack:#eng".into()]), async {
            cover_window_rpc(&cfg, req()).await
        })
        .await
        .unwrap();
        let ids: Vec<&str> = resp.value.hits.iter().map(|h| h.node_id.as_str()).collect();
        assert!(
            ids.contains(&allowed.id.as_str()),
            "allowlisted source must be present: {ids:?}"
        );
        assert!(
            !ids.contains(&blocked.id.as_str()),
            "disallowed source must be filtered out: {ids:?}"
        );

        // With no profile scope active, both sources are visible — which is what
        // makes the assertion above a filter rather than an empty store.
        let unrestricted = cover_window_rpc(&cfg, req()).await.unwrap();
        assert_eq!(unrestricted.value.hits.len(), 2);
    }

    // ── search_entities_rpc ───────────────────────────────────────────

    /// The search degrades rather than fails when the bound driver has no
    /// retrieval family, and still logs the count it served — a silent empty
    /// and a degraded empty look identical downstream otherwise.
    #[tokio::test]
    async fn search_entities_rpc_passes_through_kinds_none() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = SearchEntitiesRequest {
            query: "alice".into(),
            kinds: None,
            limit: None,
        };
        let outcome = search_entities_rpc(&cfg, req)
            .await
            .expect("a driver without the retrieval family is not an error");
        assert!(outcome.value.matches.is_empty());
        let log = &outcome.logs[0];
        assert!(log.contains("query_len=5"), "log: {log}");
        assert!(log.contains("has_kinds=false"), "log: {log}");
        assert!(log.contains("n=0"), "log: {log}");
        // PII redaction — the raw query value must NOT appear in the log.
        assert!(!log.contains("alice"), "log leaked raw query: {log}");
    }

    #[tokio::test]
    async fn search_entities_rpc_parses_valid_kinds_list() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = SearchEntitiesRequest {
            query: "x".into(),
            kinds: Some(vec!["email".into(), "topic".into()]),
            limit: Some(10),
        };
        let outcome = search_entities_rpc(&cfg, req).await.unwrap();
        assert!(outcome.value.matches.is_empty());
        assert!(
            outcome.logs[0].contains("has_kinds=true"),
            "log: {}",
            outcome.logs[0]
        );
    }

    /// An unknown kind is rejected **before** a driver is resolved, which is why
    /// this test installs no binding: a caller mistake must not need a working
    /// driver to be reported, and must not reach one and come back as an empty
    /// index instead.
    #[tokio::test]
    async fn search_entities_rpc_rejects_unknown_entity_kind() {
        let (_tmp, cfg) = test_config();
        let req = SearchEntitiesRequest {
            query: "x".into(),
            kinds: Some(vec!["email".into(), "bogus".into()]),
            limit: None,
        };
        let err = search_entities_rpc(&cfg, req).await.unwrap_err();
        assert!(err.contains("unknown entity kind: bogus"), "got {err}");
    }

    // ── drill_down_rpc ────────────────────────────────────────────────

    #[tokio::test]
    async fn drill_down_rpc_defaults_max_depth_to_one_when_unset() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = DrillDownRequest {
            node_id: "chat:missing".into(),
            max_depth: None,
            query: None,
            limit: None,
        };
        let outcome = drill_down_rpc(&cfg, req).await.unwrap();
        assert!(
            outcome.logs[0].contains("depth=1"),
            "log: {}",
            outcome.logs[0]
        );
    }

    #[tokio::test]
    async fn drill_down_rpc_logs_node_kind_prefix_for_colon_separated_id() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = DrillDownRequest {
            node_id: "chat:slack:#eng:0".into(),
            max_depth: Some(2),
            query: None,
            limit: None,
        };
        let outcome = drill_down_rpc(&cfg, req).await.unwrap();
        let log = &outcome.logs[0];
        assert!(log.contains("node_kind=chat"), "log: {log}");
        // PII redaction — scope segments beyond the kind prefix must not leak.
        assert!(!log.contains("slack"), "log leaked scope: {log}");
        assert!(!log.contains("#eng"), "log leaked scope: {log}");
    }

    #[tokio::test]
    async fn drill_down_rpc_logs_unknown_when_node_id_has_no_colon() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = DrillDownRequest {
            node_id: "rootnode".into(),
            max_depth: None,
            query: None,
            limit: None,
        };
        let outcome = drill_down_rpc(&cfg, req).await.unwrap();
        assert!(
            outcome.logs[0].contains("node_kind=unknown"),
            "log: {}",
            outcome.logs[0]
        );
    }

    /// A node id names a node, not a permission: the walk still has to run under
    /// the turn's allowlist.
    #[tokio::test]
    async fn drill_down_rpc_forwards_the_turn_scope() {
        let (_tmp, cfg) = test_config();
        let driver = bind_recording(&cfg, RecordingRetrieval::new());
        let req = DrillDownRequest {
            node_id: "chat:slack:#eng:0".into(),
            max_depth: None,
            query: None,
            limit: None,
        };
        with_source_scope(Some(vec!["slack:#eng".into()]), async {
            drill_down_rpc(&cfg, req).await.unwrap()
        })
        .await;
        let scope = driver
            .scope_for("retrieve_children")
            .expect("retrieve_children was called")
            .expect("a restricted turn must not reach the driver as unrestricted");
        assert_eq!(scope.allow, vec!["slack:#eng".to_string()]);
    }

    // ── fetch_leaves_rpc ──────────────────────────────────────────────

    #[tokio::test]
    async fn fetch_leaves_rpc_returns_empty_response_for_empty_input() {
        let (_tmp, cfg) = test_config();
        bind_without_retrieval(&cfg);
        let req = FetchLeavesRequest { chunk_ids: vec![] };
        let outcome = fetch_leaves_rpc(&cfg, req).await.unwrap();
        assert!(outcome.value.hits.is_empty());
        assert!(outcome.logs[0].contains("n=0"), "log: {}", outcome.logs[0]);
    }

    /// Naming chunk ids directly must not read around the source gate, so the
    /// scope travels with the ids.
    #[tokio::test]
    async fn fetch_leaves_rpc_returns_driver_hits_under_the_turn_scope() {
        let (_tmp, cfg) = test_config();
        let driver = bind_recording(
            &cfg,
            RecordingRetrieval::new().answering(vec![hit("chunk-a"), hit("chunk-b")]),
        );
        let req = FetchLeavesRequest {
            chunk_ids: vec!["chunk-a".into(), "chunk-b".into(), "ghost".into()],
        };
        let outcome = with_source_scope(Some(vec!["slack:#eng".into()]), async {
            fetch_leaves_rpc(&cfg, req).await.unwrap()
        })
        .await;
        assert_eq!(outcome.value.hits.len(), 2);
        assert!(outcome.logs[0].contains("n=2"), "log: {}", outcome.logs[0]);

        let scope = driver
            .scope_for("retrieve_leaves")
            .expect("retrieve_leaves was called")
            .expect("a restricted turn must not reach the driver as unrestricted");
        assert_eq!(scope.allow, vec!["slack:#eng".to_string()]);
    }

    /// `tree_kind` is `skip_serializing_if = "Option::is_none"`, so a hit that
    /// lost its kind on the way through would take the key out of the response
    /// rather than fail — the silent field loss that held this migration back
    /// while the artifact predated the field.
    #[tokio::test]
    async fn retrieval_hits_keep_tree_kind_on_the_wire() {
        let (_tmp, cfg) = test_config();
        bind_recording(
            &cfg,
            RecordingRetrieval::new().answering(vec![hit("chunk-a")]),
        );
        let outcome = fetch_leaves_rpc(
            &cfg,
            FetchLeavesRequest {
                chunk_ids: vec!["chunk-a".into()],
            },
        )
        .await
        .unwrap();
        let json = serde_json::to_value(&outcome.value).unwrap();
        assert_eq!(json["hits"][0]["tree_kind"], serde_json::json!("source"));
    }
}
