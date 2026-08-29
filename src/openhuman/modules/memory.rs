//! The host half of `ai.tinyhumans.tinymemory.Memory`.
//!
//! [`ModuleMemoryProvider`] implements `MemoryProvider` by forwarding each method
//! to the loaded module. Because the wire surface mirrors the trait one method
//! for one method, there is no translation layer here — only the bus call, the
//! error mapping, and the two decisions below.
//!
//! # Construction is synchronous and does no I/O
//!
//! `memory::binding::build` is called from `CoreContext::memory_binding`, which
//! roughly four thousand pre-boot tests invoke with no tokio runtime at all. So
//! [`ModuleMemoryProvider::new`] cannot load the module, cannot dial the bus, and
//! cannot await anything. It stores its configuration and resolves on first use,
//! the same lazy-loading contract used by the module host.
//!
//! That has one consequence worth stating plainly, because it looks like a
//! shortcut and is not:
//!
//! ## `capabilities()` is answered statically
//!
//! `MemoryProvider::capabilities` is a **synchronous** method, and the module can
//! only answer it over the bus. It therefore cannot be asked here.
//!
//! It does not need to be. The TinyMemory module serves the complete shared API,
//! and that is a property of the artifact's *source*, fixed at the version the
//! registry pins, not something to discover at runtime. So this returns
//! [`Capabilities::all`], and [`ModuleMemoryProvider::verify`] cross-checks it
//! against the module's own answer on first use and logs loudly on disagreement.
//!
//! Guessing high would be the dangerous direction: the kernel filters its RPC
//! surface and agent-tool assembly from this set, so an overstated capability
//! registers methods that answer errors. Guessing exactly is safe; the
//! cross-check catches a future artifact that widens its scope.
//!
//! # Errors round-trip through the shared table
//!
//! `tinymemory_api::wire` maps a `MemoryError` to a `(name, message)` pair and
//! back, and **both ends use it**. Reimplementing the mapping here is what would
//! let a `PathEscape` arrive as an `Invalid`, silently reclassifying a sandbox
//! escape as a caller mistake.

use std::sync::Arc;

use tinymemory_api::capabilities::{Capabilities, Capability};

/// The release whose capability set [`ARTIFACT_CAPABILITIES`] was read from.
///
/// Checked against the registry pin by `the_capability_list_matches_the_pinned_release`,
/// so bumping the pin without re-reading the list is a red test rather than a
/// silent over-claim.
pub(crate) const ARTIFACT_CAPABILITIES_PIN: &str = "1.13.3";

/// The capability families the **pinned artifact** actually serves.
///
/// Deliberately not `Capabilities::all()`. `Capability::ALL` is what the
/// *contract crate this host compiles against* declares; the loaded `cdylib` is
/// a specific release and may serve fewer families.
///
/// Re-read at tag `v1.13.3`. v1.13.0 added a `MemoryEvent` variant and two
/// additive audit fields, v1.13.1 fixed the module's source-registry path,
/// v1.13.2 fixed the `Embed` wire order, and v1.13.3 fixed folder-source path
/// resolution; none of those touched families. tinymemory#110 (in v1.13.2)
/// did add `Scoring`
/// (`ExtractEntities`, `EmbedText`, `EmbedderSlug`), which the artifact serves
/// and which `as_scoring` below forwards, so it is advertised here in the same
/// change, the way `Episodic` arrived with `as_episodic`.
///
/// Read at tag `v1.3.0`. Unchanged from v1.2.0 — the release added members
/// within existing families (`retry_failed`, the diagnostics trio,
/// `backfill_in_progress`), not families — verified with
/// `git diff v1.2.0..v1.3.0 -- crates/tinymemory-api/src/capabilities.rs`
/// returning empty. v1.2.0 is where four of the five families that v1.0.1
/// lacked arrived: `People`, `Chunks`, `Retrieval` and `Profile` all have bus
/// members there, so the under-claim that made them unreachable is over.
///
/// **`Episodic` is here in the same change that implements `as_episodic`**, as
/// the previous version of this comment required. The pinned module declares
/// the episodic methods (`InsertTurn`, `SessionTurns`, `OpenSegment`, …) and
/// [`ModuleMemoryProvider`] now forwards all of them, so the advertisement is
/// honest in both directions — the archivist writes its turns and segments
/// through this family.
///
/// **Widen this only together with the `version` bump in
/// [`super::registry`].** `the_capability_list_matches_the_pinned_release`
/// fails if the two drift.
pub(crate) const ARTIFACT_CAPABILITIES: &[Capability] = &[
    Capability::Core,
    Capability::Recall,
    Capability::Ingest,
    Capability::Documents,
    Capability::Tree,
    Capability::Entities,
    Capability::Graph,
    Capability::Diff,
    Capability::Goals,
    Capability::ToolMemory,
    Capability::Sources,
    Capability::Maintenance,
    Capability::Portability,
    // Arrived in v1.2.0. Verified against the module's declared `methods` list
    // at that tag rather than against the contract crate, which is always ahead
    // of whatever is pinned.
    Capability::People,
    Capability::Chunks,
    Capability::Retrieval,
    Capability::Profile,
    Capability::Episodic,
    // Arrived in v1.7.0 — the sync-execution and coding-session families that
    // let the host stop reaching into the engine for them. Verified against the
    // module's declared `methods` list at that tag, which serves all ten.
    Capability::SourceSync,
    Capability::CodingSessions,
    // Arrived in v1.13.2 (tinymemory#110): entity extraction, text embedding
    // and embedder identification, served by the module's engine and forwarded
    // by `MemoryScoring for ModuleMemoryProvider` below.
    Capability::Scoring,
];

/// Escape hatch for a locally-built module.
///
/// Set `OPENHUMAN_MEMORY_MODULE_ASSUME_FULL_CAPABILITIES=1` when the loaded
/// library was built from `vendor/tinymemory/crates/tinymemory-module` rather
/// than downloaded from the pinned release — that build serves the whole
/// contract, and pinning it to the older list would hide families it does have.
/// Deliberately **not** keyed off `TINYMEMORY_TEST_MODULE`: CI sets that to the
/// downloaded `v1.0.1` artifact, so keying off it would switch the guard off in
/// exactly the lane that must exercise it.
fn assume_full_capabilities() -> bool {
    matches!(
        std::env::var("OPENHUMAN_MEMORY_MODULE_ASSUME_FULL_CAPABILITIES")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// The set this build will advertise for the module driver.
fn artifact_capabilities() -> Capabilities {
    capabilities_for(assume_full_capabilities())
}

/// The advertised set for a given override state.
///
/// Split out from [`artifact_capabilities`] so the pinned-artifact invariants
/// can be asserted on the `false` branch directly. Reading the environment
/// inside the assertion would make those tests fail for anyone who has
/// `OPENHUMAN_MEMORY_MODULE_ASSUME_FULL_CAPABILITIES=1` exported — a documented,
/// supported configuration — and mutating the variable from a test would race
/// the rest of the binary.
fn capabilities_for(assume_full: bool) -> Capabilities {
    if assume_full {
        return Capabilities::all();
    }
    ARTIFACT_CAPABILITIES.iter().copied().collect()
}

/// Whether the pinned artifact serves `capability`. Drives the optional
/// `as_*()` accessors so they agree with [`artifact_capabilities`].
fn artifact_serves(capability: Capability) -> bool {
    assume_full_capabilities() || ARTIFACT_CAPABILITIES.contains(&capability)
}
use async_trait::async_trait;
use tinymemory_api::chunks::Chunk;
use tinymemory_api::error::MemoryError;
use tinymemory_api::goals::GoalsDoc;
use tinymemory_api::health::MemoryHealth;
use tinymemory_api::provider::sessions::{
    CodingSessionIngestReport, CodingSessionIngestRequest, CodingSessionSource,
};
use tinymemory_api::provider::sync::{
    RawArchiveCoverage, RawRebuildOutcome, SourceSyncState, SourceSyncStatus, SyncAuditEntry,
    SyncRunOutcome,
};
use tinymemory_api::provider::types::{
    ChunkEntityOccurrence, DiffReport, EntityHit, EntityOccurrence, ExportPage, ExportRecord,
    FlushOutcome, ForgetOutcome, ForgetSelector, ImportOutcome, IngestItem, IngestOutcome,
    MaintenanceReport, PurgeOutcome, QueueFailure, QueueStats, ResetOutcome, SnapshotRef,
    SourceItem, SourceScope, StoreStats,
};
use tinymemory_api::provider::{
    AddressBookSeedOutcome, ChunkDetail, ChunkEmbedding, ChunkListRow, ChunkQuery,
    ConversationSegment, CoverWindowQuery, Diagnosis, EntityMatch, EpisodicEvent, EpisodicTurn,
    FacetType, FastRetrieveQuery, MemoryChunks, MemoryCodingSessions, MemoryCore, MemoryDiff,
    MemoryDocuments, MemoryEntities, MemoryEpisodic, MemoryGoals, MemoryGraph, MemoryIngest,
    MemoryMaintenance, MemoryPeople, MemoryPortability, MemoryProfile, MemoryProvider,
    MemoryRecall, MemoryRetrieval, MemoryScoring, MemorySourceSink, MemorySourceSync,
    MemoryToolMemory, MemoryTree, PersonHandle, PersonInteraction, PersonRecord, PersonScore,
    ProfileFacet, RankedPerson, ResolvedPerson, RetrievalHit, RetrievalResponse,
    SourceRetrievalQuery, SourceTotal, UserState,
};
use tinymemory_api::recall::OwnedRecallOpts;
use tinymemory_api::tool_memory::ToolMemoryRule;
use tinymemory_api::tree::{IngestRequest, QueryResult, SummaryForest, TreeLeaf, TreeStatus};
use tinymemory_api::types::{
    GraphRelationRecord, MemoryCategory, MemoryEntry, MemoryKvRecord, MemoryTaint,
    NamespaceDocumentInput, NamespaceMemoryHit, NamespaceRetrievalContext, NamespaceSummary,
    StoredMemoryDocument,
};
use tinymemory_api::wire;
use tinymemory_bus::names::methods;

use super::{host, ops, registry};
use crate::openhuman::config::Config;

/// Registry id of the module these calls go to.
pub const MODULE_ID: &str = "tinymemory";

/// The `[modules]` policy this process was booted with.
///
/// # Why a process-global and not a constructor argument
///
/// `memory::binding::build` is where a module driver is constructed, and it
/// receives only a workspace dir and a `MemorySubsystemConfig`. What
/// [`ops::ensure_loaded`] needs is `modules.{enabled, allow_download,
/// install_dir}`, which lives on the full `Config` — and threading a whole
/// `Config` down through `MemoryBinding::for_workspace` would widen that
/// function's dependency and change a cache key that ~4000 pre-boot tests hit.
///
/// So the policy is published once during boot instead. This is the same shape
/// `tinymemory_core::embedding_host` and `api::product` already use, and for the
/// same stated reason: the construction sites sit too deep to thread through.
///
/// # Unset means disabled, deliberately
///
/// A pre-boot test, or a host that never called [`set_modules_policy`], gets
/// `None` — and [`policy`] then reports modules disabled rather than assuming
/// permissive defaults. Defaulting `enabled` to `true` here would silently
/// ignore an operator who turned modules off, and would let a unit test reach
/// for a download.
static MODULES_POLICY: std::sync::OnceLock<Arc<Config>> = std::sync::OnceLock::new();

/// Publish the config a module driver should load against.
///
/// Call once during boot, before any workspace is bound. Later calls are
/// ignored — a driver already resolved against the first value must not have the
/// policy change underneath it.
pub fn set_modules_policy(config: Arc<Config>) {
    let _ = MODULES_POLICY.set(config);
}

/// The published policy, if boot supplied one.
pub(crate) fn policy() -> Option<&'static Arc<Config>> {
    MODULES_POLICY.get()
}

/// A memory driver served by the loaded `tinymemory` module.
pub struct ModuleMemoryProvider {
    /// The id reported by [`MemoryProvider::driver_id`].
    driver_id: String,
    /// The config to load against, when the caller had one to give.
    ///
    /// `None` is the binding-site case: `build` has no `Config`, so the provider
    /// falls back to the policy published at boot. Tests pass one explicitly.
    config: Option<Arc<Config>>,
    /// Set once the module has answered `Capabilities`, so the cross-check runs
    /// once rather than per call.
    verified: std::sync::OnceLock<()>,
    /// Memory subtree this driver is bound to, when it is not the shared one.
    ///
    /// `None` means `<workspace>/memory` — the root object the module serves
    /// eagerly at setup. `Some("memory-<id>")` is a profile that opted into
    /// dedicated memory; the first call asks the root object to open it and
    /// caches the object path it answers with.
    memory_subdir: Option<String>,
    /// Object path resolved for [`Self::memory_subdir`], once asked for.
    resolved_path: tokio::sync::OnceCell<String>,
}

impl std::fmt::Debug for ModuleMemoryProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Config` is not rendered: it carries credentials.
        f.debug_struct("ModuleMemoryProvider")
            .field("driver_id", &self.driver_id)
            .finish_non_exhaustive()
    }
}

impl ModuleMemoryProvider {
    /// Bind the module-backed driver.
    ///
    /// Synchronous and I/O-free by requirement — see the module docs. Nothing is
    /// loaded until the first call.
    #[must_use]
    pub fn new(config: Arc<Config>) -> Self {
        Self::with_optional_config(Some(config))
    }

    /// Bind against the policy published by [`set_modules_policy`].
    ///
    /// This is what `memory::binding::build` uses, because it has no `Config` to
    /// hand over. If boot published nothing, every call reports the module
    /// unavailable rather than guessing a permissive default.
    #[must_use]
    pub fn from_boot_policy() -> Self {
        Self::with_optional_config(None)
    }

    fn with_optional_config(config: Option<Arc<Config>>) -> Self {
        Self {
            driver_id: registry::find(MODULE_ID)
                .map_or_else(|| MODULE_ID.to_string(), |record| record.id.to_string()),
            config,
            verified: std::sync::OnceLock::new(),
            memory_subdir: None,
            resolved_path: tokio::sync::OnceCell::new(),
        }
    }

    /// Bind this driver to a named memory subtree rather than the shared one.
    ///
    /// `"memory"` is the shared tree and is treated as `None`, so a caller can
    /// pass whatever `memory_subdir_for_suffix` produced without special-casing
    /// the default.
    #[must_use]
    pub fn in_subdir(mut self, memory_subdir: &str) -> Self {
        if memory_subdir != "memory" && !memory_subdir.is_empty() {
            self.memory_subdir = Some(memory_subdir.to_string());
        }
        self
    }

    /// The object path this driver talks to, opening the subtree on first use.
    ///
    /// The root object is served eagerly at module setup, so the shared tree
    /// costs nothing here. A dedicated subtree is opened once and cached; the
    /// module is idempotent per subtree, so a lost race re-uses the same store
    /// rather than opening the database twice.
    async fn object_path(&self, proxy_root: &tinybus::Proxy) -> Result<String, MemoryError> {
        let record = registry::find(MODULE_ID)
            .ok_or_else(|| MemoryError::Other(anyhow::anyhow!("unknown module '{MODULE_ID}'")))?;
        let Some(subdir) = self.memory_subdir.as_deref() else {
            return Ok(record.object_path.to_string());
        };
        self.resolved_path
            .get_or_try_init(|| async {
                log::debug!("[modules:memory] opening a dedicated memory subtree");
                proxy_root
                    .call::<String>("OpenStore", (subdir.to_string(),))
                    .await
                    .map_err(|error| from_bus(&error))
            })
            .await
            .cloned()
    }

    /// Ensure the module is serving, and hand back a proxy for its object.
    ///
    /// `operation` identifies the forwarded call (e.g. `"store"`, `"recall"`)
    /// for the diagnostic below. Never `namespace`, `key`, `content`, or any
    /// record value — those are user memory content, not correlation fields.
    async fn proxy(&self, operation: &str) -> Result<tinybus::Proxy, MemoryError> {
        log::debug!(
            "[modules:memory] driver_id={} operation={operation} resolving module proxy",
            self.driver_id,
        );
        let config = self.config.as_ref().or_else(|| policy()).ok_or_else(|| {
            MemoryError::Other(anyhow::anyhow!(
                "the module host policy was never published, so module '{MODULE_ID}' \
                 cannot be loaded; call modules::memory::set_modules_policy during boot"
            ))
        })?;
        let runtime = host::runtime().await.map_err(|error| {
            MemoryError::Other(anyhow::anyhow!("the module bus is not running: {error}"))
        })?;
        super::memory_host::install(runtime.connection(), Arc::clone(config))
            .await
            .map_err(|error| {
                MemoryError::Other(anyhow::anyhow!(
                    "the memory module host callbacks are unavailable: {error}"
                ))
            })?;
        // TinyMemory resolves its embedding provider while the native library
        // is admitted. Host callbacks must therefore exist before loading,
        // including in tests and explicit-path overrides where no boot policy
        // was available when the shared module runtime first started.
        ops::ensure_loaded(config, MODULE_ID)
            .await
            .map_err(|message| MemoryError::Other(anyhow::anyhow!(message)))?;

        let record = registry::find(MODULE_ID)
            .ok_or_else(|| MemoryError::Other(anyhow::anyhow!("unknown module '{MODULE_ID}'")))?;
        let root = runtime
            .proxy(record.bus_name, record.object_path)
            .map_err(|error| MemoryError::Other(anyhow::anyhow!(error.to_string())))?;

        self.verify(&root).await;

        // The shared tree is the root object itself, so this is a no-op for
        // every caller that did not ask for a dedicated subtree.
        let path = self.object_path(&root).await?;
        if path == record.object_path {
            return Ok(root);
        }
        runtime
            .proxy(record.bus_name, &path)
            .map_err(|error| MemoryError::Other(anyhow::anyhow!(error.to_string())))
    }

    /// Cross-check the module's advertised capabilities against what this build
    /// assumes, once per process.
    ///
    /// Compared against [`artifact_capabilities`] rather than
    /// `Capabilities::all()`: the pinned artifact answers fewer families than the
    /// contract declares (seventeen of eighteen at v1.2.0), so comparing with the
    /// full contract would warn
    /// on the *expected* state at every first module use and leave the warning
    /// permanently crying wolf. Against the configured set it fires only when
    /// the loaded artifact genuinely disagrees with the pin — including when the
    /// full-capability override is on but an older artifact was loaded.
    ///
    /// Logged rather than fatal. A mismatch means the registry pin and the
    /// artifact have diverged; the module advertising *less* than this build
    /// assumes is the dangerous direction, because the host assembles its memory
    /// RPC surface and tool families from the assumed set before any call.
    async fn verify(&self, proxy: &tinybus::Proxy) {
        if self.verified.get().is_some() {
            return;
        }
        match proxy.call::<Capabilities>("Capabilities", ()).await {
            Ok(actual) => {
                let assumed = artifact_capabilities();
                if actual != assumed {
                    log::warn!(
                        "[modules:memory] the module advertises {actual:?} but this build \
                         assumes {assumed:?}; the registry pin and the artifact have diverged"
                    );
                }
            }
            Err(error) => {
                log::warn!("[modules:memory] could not read module capabilities: {error}");
            }
        }
        let _ = self.verified.set(());
    }
}

/// Map a bus failure back onto a [`MemoryError`].
///
/// Uses the shared table so the host and the module cannot disagree about what a
/// name means. An unrecognised name becomes `Other`, never `Invalid`.
fn from_bus(error: &tinybus::Error) -> MemoryError {
    wire::from_wire(error.wire_name(), &error.to_string())
}

#[async_trait]
impl MemoryProvider for ModuleMemoryProvider {
    fn driver_id(&self) -> &str {
        &self.driver_id
    }

    /// The families the **pinned artifact** serves — not the whole contract.
    ///
    /// # This couples to the registry pin, and the coupling IS enforced
    ///
    /// `Capabilities::all()` grows whenever a family is added to the contract,
    /// but the *artifact* only grows when a release is cut and
    /// [`registry`](super::registry) is re-pinned to it. Returning `all()`
    /// between those two moments over-claimed: the host said it could do
    /// something the loaded binary could not, and [`Self::verify`] noticed and
    /// logged it without narrowing the advertised set. The failure mode was a
    /// call that reached the module and came back `UnknownMethod` (#5598)
    /// rather than a family that cleanly reported itself absent.
    ///
    /// [`ARTIFACT_CAPABILITIES`] is now the source of truth, and
    /// `the_capability_list_matches_the_pinned_release` fails if it is widened
    /// without moving [`ARTIFACT_CAPABILITIES_PIN`] and the registry pin
    /// together.
    ///
    /// The kernel filters its RPC surface and agent-tool assembly from this set,
    /// and the guard builds one family decorator per `provides()`, so an
    /// over-claim here is precisely what turns an absent family into a live
    /// method that answers `UnknownMethod`.
    fn capabilities(&self) -> Capabilities {
        artifact_capabilities()
    }

    async fn health(&self) -> MemoryHealth {
        // An unreachable module is a *health* answer, not an error: that is the
        // question this method exists to answer, and returning `Down` is how
        // status output shows an unsupported platform or a refused artifact.
        match self.proxy("health").await {
            Ok(proxy) => proxy
                .call::<MemoryHealth>("Health", ())
                .await
                .unwrap_or_else(|error| MemoryHealth::down(error.to_string())),
            Err(error) => MemoryHealth::down(error.to_string()),
        }
    }

    async fn shutdown(&self) -> Result<(), MemoryError> {
        // Deliberately does not load the module in order to shut it down: a
        // shutdown on a driver that was never used should be a no-op, not a
        // download. tinybus never unloads a library anyway, so this releases
        // backend resources only.
        if self.verified.get().is_none() {
            return Ok(());
        }
        let proxy = self.proxy("shutdown").await?;
        proxy
            .call::<()>("Shutdown", ())
            .await
            .map_err(|error| from_bus(&error))
    }

    fn as_ingest(&self) -> Option<&dyn MemoryIngest> {
        Some(self)
    }
    fn as_documents(&self) -> Option<&dyn MemoryDocuments> {
        Some(self)
    }
    fn as_tree(&self) -> Option<&dyn MemoryTree> {
        Some(self)
    }
    fn as_entities(&self) -> Option<&dyn MemoryEntities> {
        Some(self)
    }
    fn as_graph(&self) -> Option<&dyn MemoryGraph> {
        Some(self)
    }
    fn as_diff(&self) -> Option<&dyn MemoryDiff> {
        Some(self)
    }
    fn as_goals(&self) -> Option<&dyn MemoryGoals> {
        Some(self)
    }
    fn as_tool_memory(&self) -> Option<&dyn MemoryToolMemory> {
        Some(self)
    }
    fn as_sources(&self) -> Option<&dyn MemorySourceSink> {
        Some(self)
    }
    fn as_maintenance(&self) -> Option<&dyn MemoryMaintenance> {
        Some(self)
    }
    // The four families below are gated on the pinned artifact rather than
    // returning `Some(self)` unconditionally. `provides()` derives from these
    // accessors, the guard builds its decorators from `provides()`, and every
    // caller already writes a clean "driver does not support the X family"
    // error on `None` — so gating here converts a deep `UnknownMethod` into an
    // early, accurate refusal at every call site at once (#5598).
    fn as_people(&self) -> Option<&dyn MemoryPeople> {
        artifact_serves(Capability::People).then_some(self as &dyn MemoryPeople)
    }
    fn as_chunks(&self) -> Option<&dyn MemoryChunks> {
        artifact_serves(Capability::Chunks).then_some(self as &dyn MemoryChunks)
    }
    fn as_retrieval(&self) -> Option<&dyn MemoryRetrieval> {
        artifact_serves(Capability::Retrieval).then_some(self as &dyn MemoryRetrieval)
    }
    fn as_profile(&self) -> Option<&dyn MemoryProfile> {
        artifact_serves(Capability::Profile).then_some(self as &dyn MemoryProfile)
    }

    fn as_source_sync(&self) -> Option<&dyn MemorySourceSync> {
        artifact_serves(Capability::SourceSync).then_some(self as &dyn MemorySourceSync)
    }

    fn as_coding_sessions(&self) -> Option<&dyn MemoryCodingSessions> {
        artifact_serves(Capability::CodingSessions).then_some(self as &dyn MemoryCodingSessions)
    }
    fn as_episodic(&self) -> Option<&dyn MemoryEpisodic> {
        artifact_serves(Capability::Episodic).then_some(self as &dyn MemoryEpisodic)
    }
    fn as_scoring(&self) -> Option<&dyn MemoryScoring> {
        artifact_serves(Capability::Scoring).then_some(self as &dyn MemoryScoring)
    }
}

#[async_trait]
impl MemoryCore for ModuleMemoryProvider {
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        // No log line carries `namespace`, `key` or `content`: all three are user
        // memory content.
        self.proxy("store")
            .await?
            .call::<()>(
                "Store",
                (
                    namespace,
                    key,
                    content,
                    category,
                    session_id.map(str::to_string),
                    taint,
                ),
            )
            .await
            .map_err(|error| from_bus(&error))
    }

    async fn get(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>, MemoryError> {
        self.proxy("get")
            .await?
            .call("Get", (namespace, key))
            .await
            .map_err(|error| from_bus(&error))
    }

    async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
        self.proxy("forget")
            .await?
            .call("Forget", (namespace, key))
            .await
            .map_err(|error| from_bus(&error))
    }

    async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<&MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.proxy("list")
            .await?
            .call(
                "List",
                (
                    namespace.map(str::to_string),
                    category.cloned(),
                    session_id.map(str::to_string),
                ),
            )
            .await
            .map_err(|error| from_bus(&error))
    }

    async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        self.proxy("namespaces")
            .await?
            .call("Namespaces", ())
            .await
            .map_err(|error| from_bus(&error))
    }
}

#[async_trait]
impl MemoryRecall for ModuleMemoryProvider {
    async fn recall(
        &self,
        query: &str,
        limit: usize,
        opts: &OwnedRecallOpts,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        // `scope` crosses as a value because the driver must apply it as a query
        // predicate internally; narrowing the result here instead would let the
        // module spend its `limit` on entries the caller may not see.
        self.proxy("recall")
            .await?
            .call("Recall", (query, limit, opts, scope.cloned()))
            .await
            .map_err(|error| from_bus(&error))
    }
}

#[async_trait]
impl MemoryPortability for ModuleMemoryProvider {
    async fn export_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ExportPage, MemoryError> {
        self.proxy("export_page")
            .await?
            .call("ExportPage", (cursor.map(str::to_string), limit))
            .await
            .map_err(|error| from_bus(&error))
    }

    async fn import_records(
        &self,
        records: Vec<ExportRecord>,
    ) -> Result<ImportOutcome, MemoryError> {
        self.proxy("import_records")
            .await?
            .call("ImportRecords", (records,))
            .await
            .map_err(|error| from_bus(&error))
    }
}

macro_rules! module_call {
    ($self:expr, $operation:literal, $method:expr, $args:expr) => {
        $self
            .proxy($operation)
            .await?
            .call($method, $args)
            .await
            .map_err(|error| from_bus(&error))
    };
}

#[async_trait]
impl MemoryIngest for ModuleMemoryProvider {
    async fn ingest_document(&self, item: IngestItem) -> Result<IngestOutcome, MemoryError> {
        module_call!(self, "ingest_document", "IngestDocument", (item,))
    }
    async fn ingest_chat(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        module_call!(self, "ingest_chat", "IngestChat", (messages,))
    }
    async fn ingest_email(&self, messages: Vec<IngestItem>) -> Result<IngestOutcome, MemoryError> {
        module_call!(self, "ingest_email", "IngestEmail", (messages,))
    }
}

#[async_trait]
impl MemoryDocuments for ModuleMemoryProvider {
    async fn put_document(&self, input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        module_call!(self, "put_document", "PutDocument", (input,))
    }
    async fn get_document(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        module_call!(self, "get_document", "GetDocument", (namespace, key))
    }
    async fn list_documents(
        &self,
        namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError> {
        module_call!(
            self,
            "list_documents",
            "ListDocuments",
            (namespace.map(str::to_string),)
        )
    }
    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        module_call!(self, "list_namespaces", "ListNamespaces", ())
    }
    async fn delete_document(
        &self,
        namespace: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, MemoryError> {
        module_call!(
            self,
            "delete_document",
            "DeleteDocument",
            (namespace, document_id)
        )
    }
    async fn clear_namespace(&self, namespace: &str) -> Result<(), MemoryError> {
        module_call!(self, "clear_namespace", "ClearNamespace", (namespace,))
    }
    async fn query_documents(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        module_call!(
            self,
            "query_documents",
            "QueryDocuments",
            (namespace, query, limit)
        )
    }
    async fn recall_documents(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        module_call!(
            self,
            "recall_documents",
            "RecallDocuments",
            (namespace, limit)
        )
    }
}

#[async_trait]
impl MemoryTree for ModuleMemoryProvider {
    async fn append(&self, request: IngestRequest) -> Result<(), MemoryError> {
        module_call!(self, "append", "Append", (request,))
    }
    async fn query_source(
        &self,
        namespace: &str,
        source_id: &str,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        module_call!(
            self,
            "query_source",
            "QuerySource",
            (namespace, source_id, limit, scope.cloned())
        )
    }
    async fn drill_down(&self, namespace: &str, node_id: &str) -> Result<QueryResult, MemoryError> {
        module_call!(self, "drill_down", "DrillDown", (namespace, node_id))
    }
    async fn seal(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        module_call!(self, "seal", "Seal", (namespace,))
    }
    async fn cascade(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        module_call!(self, "cascade", "Cascade", (namespace,))
    }
    async fn summary_forest(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<SummaryForest, MemoryError> {
        module_call!(self, "summary_forest", "SummaryForest", (limit, scope))
    }

    async fn flush_source_tree(&self, source_scope: &str) -> Result<u64, MemoryError> {
        module_call!(
            self,
            "flush_source_tree",
            "FlushSourceTree",
            (source_scope,)
        )
    }
    async fn recent_leaves(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<TreeLeaf>, MemoryError> {
        module_call!(self, "recent_leaves", "RecentLeaves", (limit, scope))
    }
}

#[async_trait]
impl MemoryEntities for ModuleMemoryProvider {
    async fn entities(
        &self,
        namespace: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError> {
        module_call!(
            self,
            "entities",
            "Entities",
            (namespace, query.map(str::to_string), limit)
        )
    }
    async fn entity_edges(
        &self,
        namespace: &str,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        module_call!(
            self,
            "entity_edges",
            "EntityEdges",
            (namespace, entity_id, limit)
        )
    }
    async fn touch_entities(
        &self,
        namespace: &str,
        entity_ids: &[String],
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "touch_entities",
            "TouchEntities",
            (namespace, entity_ids.to_vec())
        )
    }
    async fn top_entities(
        &self,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityOccurrence>, MemoryError> {
        module_call!(self, "top_entities", "TopEntities", (kind, limit))
    }
    async fn chunk_entities(
        &self,
        chunk_ids: &[String],
        kinds: Option<&[String]>,
    ) -> Result<Vec<ChunkEntityOccurrence>, MemoryError> {
        module_call!(self, "chunk_entities", "ChunkEntities", (chunk_ids, kinds))
    }
    async fn entity_chunk_ids(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, MemoryError> {
        module_call!(
            self,
            "entity_chunk_ids",
            "EntityChunkIds",
            (entity_id, limit)
        )
    }
}

#[async_trait]
impl MemoryGraph for ModuleMemoryProvider {
    async fn kv_get(
        &self,
        namespace: Option<&str>,
        key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        module_call!(
            self,
            "kv_get",
            "KvGet",
            (namespace.map(str::to_string), key)
        )
    }
    async fn kv_put(
        &self,
        namespace: Option<&str>,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "kv_put",
            "KvPut",
            (namespace.map(str::to_string), key, value)
        )
    }
    async fn kv_delete(&self, namespace: Option<&str>, key: &str) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "kv_delete",
            "KvDelete",
            (namespace.map(str::to_string), key)
        )
    }
    async fn kv_list(
        &self,
        namespace: Option<&str>,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        module_call!(
            self,
            "kv_list",
            "KvList",
            (
                namespace.map(str::to_string),
                prefix.map(str::to_string),
                limit
            )
        )
    }
    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        module_call!(
            self,
            "relations",
            "Relations",
            (
                namespace.map(str::to_string),
                subject.map(str::to_string),
                predicate.map(str::to_string),
                limit
            )
        )
    }
    async fn put_relation(&self, relation: GraphRelationRecord) -> Result<(), MemoryError> {
        module_call!(self, "put_relation", "PutRelation", (relation,))
    }
}

#[async_trait]
impl MemoryDiff for ModuleMemoryProvider {
    async fn capture_snapshot(&self, source_id: &str) -> Result<SnapshotRef, MemoryError> {
        module_call!(self, "capture_snapshot", "CaptureSnapshot", (source_id,))
    }
    async fn snapshots(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError> {
        module_call!(self, "snapshots", "Snapshots", (source_id, limit))
    }
    async fn diff(
        &self,
        source_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<DiffReport, MemoryError> {
        module_call!(
            self,
            "diff",
            "Diff",
            (source_id, from.map(str::to_string), to)
        )
    }
}

#[async_trait]
impl MemoryGoals for ModuleMemoryProvider {
    async fn goals(&self) -> Result<GoalsDoc, MemoryError> {
        module_call!(self, "goals", "Goals", ())
    }
    async fn set_goals(&self, goals: GoalsDoc) -> Result<(), MemoryError> {
        module_call!(self, "set_goals", "SetGoals", (goals,))
    }
}

#[async_trait]
impl MemoryToolMemory for ModuleMemoryProvider {
    async fn tool_rules(&self, tool_name: &str) -> Result<Vec<ToolMemoryRule>, MemoryError> {
        module_call!(self, "tool_rules", "ToolRules", (tool_name,))
    }
    async fn put_tool_rule(&self, rule: ToolMemoryRule) -> Result<(), MemoryError> {
        module_call!(self, "put_tool_rule", "PutToolRule", (rule,))
    }
    async fn delete_tool_rule(&self, tool_name: &str, rule_id: &str) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "delete_tool_rule",
            "DeleteToolRule",
            (tool_name, rule_id)
        )
    }
}

#[async_trait]
impl MemorySourceSink for ModuleMemoryProvider {
    async fn accept_source_items(
        &self,
        source_id: &str,
        source_kind: &str,
        items: Vec<SourceItem>,
        taint: MemoryTaint,
    ) -> Result<IngestOutcome, MemoryError> {
        module_call!(
            self,
            "accept_source_items",
            "AcceptSourceItems",
            (source_id, source_kind, items, taint)
        )
    }
    async fn forget_source(&self, source_id: &str) -> Result<u64, MemoryError> {
        module_call!(self, "forget_source", "ForgetSource", (source_id,))
    }
    async fn forget_matching(
        &self,
        selector: &ForgetSelector,
    ) -> Result<ForgetOutcome, MemoryError> {
        module_call!(self, "forget_matching", "ForgetMatching", (selector,))
    }
}

#[async_trait]
impl MemoryMaintenance for ModuleMemoryProvider {
    async fn reembed(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "reembed", "Reembed", ())
    }
    async fn compact(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "compact", "Compact", ())
    }
    async fn consolidate(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "consolidate", "Consolidate", ())
    }
    async fn doctor(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "doctor", "Doctor", ())
    }
    async fn retry_failed(&self) -> Result<MaintenanceReport, MemoryError> {
        module_call!(self, "retry_failed", "RetryFailed", ())
    }
    async fn store_stats(&self) -> Result<StoreStats, MemoryError> {
        module_call!(self, "store_stats", "StoreStats", ())
    }
    async fn queue_stats(&self, kind: Option<&str>) -> Result<QueueStats, MemoryError> {
        module_call!(self, "queue_stats", "QueueStats", (kind,))
    }
    async fn latest_queue_failure(&self) -> Result<Option<QueueFailure>, MemoryError> {
        module_call!(self, "latest_queue_failure", "LatestQueueFailure", ())
    }
    async fn backfill_in_progress(&self) -> Result<bool, MemoryError> {
        module_call!(self, "backfill_in_progress", "BackfillInProgress", ())
    }
    async fn flush_pending(&self) -> Result<FlushOutcome, MemoryError> {
        module_call!(self, "flush_pending", "FlushPending", ())
    }
    async fn reset_derived_index(&self) -> Result<ResetOutcome, MemoryError> {
        module_call!(self, "reset_derived_index", "ResetDerivedIndex", ())
    }
    async fn purge_all(&self) -> Result<PurgeOutcome, MemoryError> {
        module_call!(self, "purge_all", "PurgeAll", ())
    }
    async fn diagnose(&self) -> Result<Diagnosis, MemoryError> {
        module_call!(self, "diagnose", "Diagnose", ())
    }
}

/// Bus deadline for the three calls that run a whole source sync inside the
/// module: `RunConnectionSync`, `RunSourceSync` and `BootstrapConnection`.
///
/// tinybus gives every call a 30 s default deadline if nobody sets one, and a
/// sync is routinely longer than that: one Gmail page is ~31 s end to end, an
/// initial bootstrap of a connection is minutes. With the default, the caller
/// was released with "call to `RunSourceSync` timed out after 30000ms" while
/// the module kept fetching and ingesting, and finished; the UI reported a
/// failure for work that succeeded (openhuman#5820). Same failure class, same
/// fix as `IngestCodingSessions` above: the deadline here is the wedged-forever
/// backstop tinybus requires, not a ceiling anyone is meant to hit.
///
/// Sized from the frontend's clamp, `PER_CALL_TIMEOUT_MAX_MS = 600 s`
/// (`app/src/services/coreRpcClient.ts`): that is the longest wait any RPC
/// caller can observe, so the bus must outlast it, plus [`INGEST_BUS_GRACE`]
/// so the client's own abort, with its clean message, is the one that fires
/// first when a run really does wedge.
const SOURCE_SYNC_BUS_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(600).saturating_add(INGEST_BUS_GRACE);

#[async_trait]
impl MemorySourceSync for ModuleMemoryProvider {
    async fn run_connection_sync(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<SyncRunOutcome, MemoryError> {
        self.proxy("run_connection_sync")
            .await?
            .with_timeout(SOURCE_SYNC_BUS_TIMEOUT)
            .call("RunConnectionSync", (toolkit, connection_id))
            .await
            .map_err(|error| from_bus(&error))
    }
    async fn run_source_sync(&self, source_id: &str) -> Result<SyncRunOutcome, MemoryError> {
        self.proxy("run_source_sync")
            .await?
            .with_timeout(SOURCE_SYNC_BUS_TIMEOUT)
            .call("RunSourceSync", (source_id,))
            .await
            .map_err(|error| from_bus(&error))
    }
    async fn bootstrap_connection(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<(), MemoryError> {
        self.proxy("bootstrap_connection")
            .await?
            .with_timeout(SOURCE_SYNC_BUS_TIMEOUT)
            .call("BootstrapConnection", (toolkit, connection_id))
            .await
            .map_err(|error| from_bus(&error))
    }
    async fn is_toolkit_syncable(&self, toolkit: &str) -> Result<bool, MemoryError> {
        module_call!(self, "is_toolkit_syncable", "IsToolkitSyncable", (toolkit,))
    }
    async fn source_sync_state(
        &self,
        toolkit: &str,
        connection_id: &str,
    ) -> Result<Option<SourceSyncState>, MemoryError> {
        module_call!(
            self,
            "source_sync_state",
            "SourceSyncState",
            (toolkit, connection_id)
        )
    }
    async fn sync_audit_log(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<SyncAuditEntry>, MemoryError> {
        module_call!(self, "sync_audit_log", "SyncAuditLog", (limit,))
    }
    async fn estimate_sync_cost_usd(
        &self,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<f64, MemoryError> {
        module_call!(
            self,
            "estimate_sync_cost_usd",
            "EstimateSyncCostUsd",
            (input_tokens, output_tokens)
        )
    }
    async fn sync_statuses(&self) -> Result<Vec<SourceSyncStatus>, MemoryError> {
        module_call!(self, "sync_statuses", "SyncStatuses", ())
    }
    async fn raw_archive_coverage(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawArchiveCoverage, MemoryError> {
        module_call!(
            self,
            "raw_archive_coverage",
            "RawArchiveCoverage",
            (tree_scope, archive_source_id)
        )
    }
    async fn rebuild_from_raw_archive(
        &self,
        tree_scope: &str,
        archive_source_id: &str,
    ) -> Result<RawRebuildOutcome, MemoryError> {
        module_call!(
            self,
            "rebuild_from_raw_archive",
            "RebuildFromRawArchive",
            (tree_scope, archive_source_id)
        )
    }
}

#[async_trait]
impl MemoryCodingSessions for ModuleMemoryProvider {
    async fn coding_session_status(&self) -> Result<Vec<CodingSessionSource>, MemoryError> {
        module_call!(self, "coding_session_status", "CodingSessionStatus", ())
    }
    /// # Why this one call sets its own bus deadline
    ///
    /// Every other member here takes tinybus' `DEFAULT_TIMEOUT`
    /// (`vendor/tinybus/crates/tinybus/src/connection.rs:56`) — a flat 30 s,
    /// applied by `Proxy::new` (`proxy.rs:59`) whenever nobody says otherwise.
    /// That is the right default for a memory read. It is the wrong one here:
    /// distilling a coding session is several *sequential* model calls, and the
    /// RPC above it already computes a budget sized to the work
    /// (`memory::sources::rpc::ingest_budget`, 120 s + 90 s per session, capped
    /// at 600 s).
    ///
    /// So there were two deadlines and the tighter one was the one nobody
    /// chose. A real 35 s import tripped the 30 s default; the caller was
    /// released with an error while the module kept working and finished
    /// seconds later, having imported everything. The UI reported a failure for
    /// work that had succeeded, and invited a retry that would redo it
    /// (#5802).
    ///
    /// tinybus is explicit that this is the caller's problem to size: *"A
    /// timeout does not cancel the remote work — tinybus cannot — it stops
    /// waiting and frees the caller"* (`connection.rs:22-23`). Abandoning the
    /// call early therefore does not save anything; it only loses the report.
    ///
    /// The budget is taken from `ingest_budget` rather than restated, so the
    /// two layers cannot drift, plus [`INGEST_BUS_GRACE`]. The grace makes the
    /// ordering deterministic instead of a race between two equal deadlines:
    /// the RPC's own `tokio::time::timeout` fires first and reports its clean
    /// structured message, and this deadline survives only as the
    /// wedged-forever backstop tinybus requires. Same shape as the client's
    /// `CODING_SESSION_RPC_GRACE_MS` sitting above the server budget.
    async fn ingest_coding_sessions(
        &self,
        request: CodingSessionIngestRequest,
    ) -> Result<CodingSessionIngestReport, MemoryError> {
        let deadline = crate::openhuman::memory::sources::rpc::ingest_budget(request.max_sessions)
            + INGEST_BUS_GRACE;
        self.proxy("ingest_coding_sessions")
            .await?
            .with_timeout(deadline)
            .call("IngestCodingSessions", (request,))
            .await
            .map_err(|error| from_bus(&error))
    }
}

/// Head-room added to [`ingest_budget`](crate::openhuman::memory::sources::rpc::ingest_budget)
/// for the bus deadline on `IngestCodingSessions`.
///
/// Exists to order two deadlines, not to allow more work: the RPC's own
/// wall-clock ceiling must be the one that fires, because its message names the
/// budget rather than the wire member. Anything comfortably longer than the
/// scheduling jitter between the two `tokio::time::timeout` arms would do.
const INGEST_BUS_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

#[async_trait]
impl MemoryEpisodic for ModuleMemoryProvider {
    async fn insert_turn(&self, turn: &EpisodicTurn) -> Result<i64, MemoryError> {
        module_call!(self, "insert_turn", "InsertTurn", (turn,))
    }
    async fn session_turns(&self, session_id: &str) -> Result<Vec<EpisodicTurn>, MemoryError> {
        module_call!(self, "session_turns", "SessionTurns", (session_id,))
    }
    async fn open_segment(
        &self,
        session_id: &str,
    ) -> Result<Option<ConversationSegment>, MemoryError> {
        module_call!(self, "open_segment", "OpenSegment", (session_id,))
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "trait signature; see the contract's rationale"
    )]
    async fn create_segment(
        &self,
        segment_id: &str,
        session_id: &str,
        namespace: &str,
        start_episodic_id: i64,
        start_seq: Option<u32>,
        start_timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "create_segment",
            "CreateSegment",
            (
                segment_id,
                session_id,
                namespace,
                start_episodic_id,
                start_seq,
                start_timestamp,
                now
            )
        )
    }
    async fn append_turn(
        &self,
        segment_id: &str,
        episodic_id: i64,
        seq: Option<u32>,
        timestamp: f64,
        now: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "append_turn",
            "AppendTurn",
            (segment_id, episodic_id, seq, timestamp, now)
        )
    }
    async fn insert_event(&self, event: &EpisodicEvent) -> Result<(), MemoryError> {
        module_call!(self, "insert_event", "InsertEvent", (event,))
    }
    async fn close_segment(&self, segment_id: &str, now: f64) -> Result<(), MemoryError> {
        module_call!(self, "close_segment", "CloseSegment", (segment_id, now))
    }
    async fn set_segment_summary(
        &self,
        segment_id: &str,
        summary: &str,
        now: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "set_segment_summary",
            "SetSegmentSummary",
            (segment_id, summary, now)
        )
    }
    async fn upsert_segment_embedding(
        &self,
        segment_id: &str,
        model_signature: &str,
        embedding: &[f32],
        created_at: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "upsert_segment_embedding",
            "UpsertSegmentEmbedding",
            (segment_id, model_signature, embedding, created_at)
        )
    }
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;

#[async_trait]
impl MemoryPeople for ModuleMemoryProvider {
    async fn list_people(&self, limit: Option<usize>) -> Result<Vec<RankedPerson>, MemoryError> {
        module_call!(self, "list_people", "ListPeople", (limit,))
    }
    async fn get_person(&self, person_id: &str) -> Result<Option<PersonRecord>, MemoryError> {
        module_call!(self, "get_person", "GetPerson", (person_id,))
    }
    async fn resolve_handle(
        &self,
        handle: &PersonHandle,
        create_if_missing: bool,
    ) -> Result<Option<ResolvedPerson>, MemoryError> {
        module_call!(
            self,
            "resolve_handle",
            "ResolveHandle",
            (handle, create_if_missing)
        )
    }
    async fn add_handle_alias(
        &self,
        person_id: &str,
        handle: &PersonHandle,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "add_handle_alias",
            "AddHandleAlias",
            (person_id, handle)
        )
    }
    async fn score_person(&self, person_id: &str) -> Result<Option<PersonScore>, MemoryError> {
        module_call!(self, "score_person", "ScorePerson", (person_id,))
    }
    async fn record_interaction(&self, interaction: &PersonInteraction) -> Result<(), MemoryError> {
        module_call!(
            self,
            "record_interaction",
            "RecordInteraction",
            (interaction,)
        )
    }
    async fn seed_from_address_book(&self) -> Result<AddressBookSeedOutcome, MemoryError> {
        module_call!(self, "seed_from_address_book", "SeedFromAddressBook", ())
    }
}

#[async_trait]
impl MemoryChunks for ModuleMemoryProvider {
    async fn list_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        module_call!(self, "list_chunks", "ListChunks", (query, scope))
    }
    async fn get_chunk(&self, chunk_id: &str) -> Result<Option<Chunk>, MemoryError> {
        module_call!(self, "get_chunk", "GetChunk", (chunk_id,))
    }
    async fn chunk_detail(&self, chunk_id: &str) -> Result<Option<ChunkDetail>, MemoryError> {
        module_call!(self, "chunk_detail", "ChunkDetail", (chunk_id,))
    }
    async fn storage_kinds(&self) -> Result<Vec<String>, MemoryError> {
        module_call!(self, "storage_kinds", "StorageKinds", ())
    }
    async fn chunk_embeddings(
        &self,
        chunk_ids: &[String],
        model_signature: &str,
    ) -> Result<Vec<ChunkEmbedding>, MemoryError> {
        module_call!(
            self,
            "chunk_embeddings",
            "ChunkEmbeddings",
            (chunk_ids, model_signature)
        )
    }
    async fn count_chunks(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<u64, MemoryError> {
        module_call!(self, "count_chunks", "CountChunks", (query, scope))
    }
    async fn list_chunk_details(
        &self,
        query: &ChunkQuery,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<ChunkListRow>, MemoryError> {
        module_call!(
            self,
            "list_chunk_details",
            "ListChunkDetails",
            (query, scope)
        )
    }
    async fn source_totals(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<SourceTotal>, MemoryError> {
        module_call!(self, "source_totals", "SourceTotals", (limit, scope))
    }
}

#[async_trait]
impl MemoryRetrieval for ModuleMemoryProvider {
    async fn fast_retrieve(
        &self,
        query: &str,
        options: FastRetrieveQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        module_call!(
            self,
            "fast_retrieve",
            "FastRetrieve",
            (query, options, scope)
        )
    }
    async fn cover_window(
        &self,
        window: &CoverWindowQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        module_call!(self, "cover_window", "CoverWindow", (window, scope))
    }
    async fn retrieve_source(
        &self,
        query: &SourceRetrievalQuery,
        scope: Option<&SourceScope>,
    ) -> Result<RetrievalResponse, MemoryError> {
        module_call!(self, "retrieve_source", "RetrieveSource", (query, scope))
    }
    async fn retrieve_children(
        &self,
        node_id: &str,
        max_depth: u32,
        query: Option<&str>,
        limit: Option<usize>,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        module_call!(
            self,
            "retrieve_children",
            "RetrieveChildren",
            (node_id, max_depth, query, limit, scope)
        )
    }
    async fn retrieve_leaves(
        &self,
        chunk_ids: &[String],
        scope: Option<&SourceScope>,
    ) -> Result<Vec<RetrievalHit>, MemoryError> {
        module_call!(
            self,
            "retrieve_leaves",
            "RetrieveLeaves",
            (chunk_ids, scope)
        )
    }
    async fn recall_namespace_recent(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        module_call!(
            self,
            "recall_namespace_recent",
            "RecallNamespaceRecent",
            (namespace, limit)
        )
    }
    async fn recall_namespace_scored(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
        exclude_session_id: Option<&str>,
    ) -> Result<Vec<NamespaceMemoryHit>, MemoryError> {
        module_call!(
            self,
            "recall_namespace_scored",
            "RecallNamespaceScored",
            (namespace, query, limit, exclude_session_id)
        )
    }
    async fn search_entities(
        &self,
        query: &str,
        kinds: Option<&[String]>,
        limit: usize,
    ) -> Result<Vec<EntityMatch>, MemoryError> {
        module_call!(
            self,
            "search_entities",
            "SearchEntities",
            (query, kinds, limit)
        )
    }
}

#[async_trait]
impl MemoryProfile for ModuleMemoryProvider {
    async fn list_active_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        module_call!(self, "list_active_facets", "ListActiveFacets", ())
    }
    async fn list_all_facets(&self) -> Result<Vec<ProfileFacet>, MemoryError> {
        module_call!(self, "list_all_facets", "ListAllFacets", ())
    }
    async fn get_facet(&self, key: &str) -> Result<Option<ProfileFacet>, MemoryError> {
        module_call!(self, "get_facet", "GetFacet", (key,))
    }
    async fn facets_by_type(
        &self,
        facet_type: FacetType,
    ) -> Result<Vec<ProfileFacet>, MemoryError> {
        module_call!(self, "facets_by_type", "FacetsByType", (facet_type,))
    }
    async fn upsert_facet(&self, facet: &ProfileFacet) -> Result<(), MemoryError> {
        module_call!(self, "upsert_facet", "UpsertFacet", (facet,))
    }
    async fn upsert_provider_facet(
        &self,
        facet_id: &str,
        facet_type: FacetType,
        key: &str,
        value: &str,
        confidence: f64,
        segment_id: Option<&str>,
        observed_at: f64,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "upsert_provider_facet",
            "UpsertProviderFacet",
            (
                facet_id,
                facet_type,
                key,
                value,
                confidence,
                segment_id,
                observed_at
            )
        )
    }
    async fn set_facet_user_state(
        &self,
        key: &str,
        user_state: UserState,
    ) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "set_facet_user_state",
            "SetFacetUserState",
            (key, user_state)
        )
    }
    async fn delete_facet(&self, key: &str) -> Result<bool, MemoryError> {
        module_call!(self, "delete_facet", "DeleteFacet", (key,))
    }
    async fn delete_facet_by_id(&self, facet_id: &str) -> Result<bool, MemoryError> {
        module_call!(self, "delete_facet_by_id", "DeleteFacetById", (facet_id,))
    }
    async fn drop_facets_below(&self, threshold: f64) -> Result<usize, MemoryError> {
        module_call!(self, "drop_facets_below", "DropFacetsBelow", (threshold,))
    }
    /// Any transport failure reads as `false` — the trait's documented rule for
    /// this predicate, and the reason it returns `bool` rather than a `Result`.
    async fn workflow_identity_matches(&self, key_pattern: &str, canonical_value: &str) -> bool {
        // Written out rather than via `module_call!`: that macro uses `?`, which
        // needs a `Result`-returning body, and this one returns `bool` on
        // purpose. Both failure points — resolving the proxy and the call
        // itself — collapse to `false`, which is the rule above.
        let Ok(proxy) = self.proxy("workflow_identity_matches").await else {
            return false;
        };
        proxy
            .call::<bool>("WorkflowIdentityMatches", (key_pattern, canonical_value))
            .await
            .unwrap_or(false)
    }
}

#[async_trait]
impl MemoryScoring for ModuleMemoryProvider {
    async fn extract_entities(&self, query: &str) -> Result<Vec<String>, MemoryError> {
        module_call!(
            self,
            "extract_entities",
            methods::EXTRACT_ENTITIES,
            (query,)
        )
    }
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, MemoryError> {
        module_call!(self, "embed_text", methods::EMBED_TEXT, (text,))
    }
    async fn embedder_slug(&self) -> Result<String, MemoryError> {
        module_call!(self, "embedder_slug", methods::EMBEDDER_SLUG, ())
    }
}
