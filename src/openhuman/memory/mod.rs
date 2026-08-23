//! Memory orchestration — the **host layer** over `tinymemory-core`.
//!
//! The substance of the memory subsystem was extracted into
//! [`tinymemory_core`]: the SQLite/vector store, the markdown summary tree, the
//! provider sync pipelines, ingestion, recall/query/search, the ingest queue,
//! conversations, people, goals and the tool-memory rules. That crate names no
//! OpenHuman type.
//!
//! What stays here is the host layer, per the tinymemory README's split:
//!
//! | Module | Role |
//! | ------ | ---- |
//! | [`schemas`] / [`read_rpc`] | the JSON-RPC surface and its controller registration |
//! | [`tools`] | the memory agent tools |
//! | [`guard`] | the taint/scope/budget policy gate over every provider call |
//! | [`driver`] | driver binding — which provider backs this workspace |
//! | [`ops`] | RPC handlers, delegating into the core |
//! | [`agent`] | the memory agent and its prompt |
//! | [`global`] | the per-workspace singleton |
//! | [`host`] | the seam impls — [`host::install_memory_event_sink`] and `MemoryHostConfig for Config` |
//!
//! Everything else in this module is a **re-export** of the extracted crate, so
//! the ~550 `crate::openhuman::memory::…` paths elsewhere in this crate keep
//! resolving unchanged. Prefer `tinymemory_core::…` in new code.

// ── The gate ───────────────────────────────────────────────────────────────
//
// FACADE + STUB, the `voice` shape with `skills`' refinement. `pub mod memory;`
// in `openhuman/mod.rs` is always compiled; the behavioural submodules below
// are `#[cfg(feature = "memory")]`, and `stub.rs` re-exposes the surface that
// always-on callers reach — the agent harness, flows, channels, cron, the
// controller registry — with null / disabled bodies.
//
// TYPE CARVE-OUT: `api`, `ranking` and `source_scope` are compiled in BOTH
// builds and are deliberately NOT in this list. The first is the contract
// vocabulary (a re-export of `tinymemory-api`), the second is host arithmetic
// with no driver behind it, and the third is host policy that crosses the bus
// as a value. None of them can fail without a driver, and a stub twin of any
// of them would be a second definition free to drift — the mistake `skills`'
// carve-out exists to avoid.
#[cfg(feature = "memory")]
pub mod agent;
pub mod api;
#[cfg(feature = "memory")]
pub mod binding;
/// Memory-recall provenance (`MemoryCitation`, `CROSS_CHAT_HEADER`) — inert
/// serde types that appear in always-compiled agent signatures, so they are a
/// type carve-out rather than part of the gated behaviour.
pub mod citation;
#[cfg(feature = "memory")]
pub mod driver;
#[cfg(feature = "memory")]
pub mod guard;
/// The host side of the `tinymemory` seam — `MemoryHostConfig for Config` and
/// the bus event sink. **Ungated**, and deliberately so: these are impls of
/// `tinymemory_api::host` traits on OpenHuman's own `Config`, named by
/// always-compiled callers (composio, task-sources, the doctor, the tool
/// registry) that pass a `&Config` where the API crate's `dyn MemoryHostConfig`
/// is expected. Gating the impl would make `Config` stop satisfying a bound
/// that has nothing to do with whether the memory *family* is compiled in.
pub mod host;
#[cfg(feature = "memory")]
pub mod host_impls;
/// Well-known memory namespace names. Ungated for the same reason as
/// [`citation`]: inert strings that always-compiled readers name.
pub mod namespaces;
#[cfg(feature = "memory")]
pub mod ops;
#[cfg(feature = "memory")]
pub mod preferences;
#[cfg(feature = "memory")]
pub mod sync_events_bridge;
// The consolidated `memory_query` agent tool and its six retrieval modes. Came
// back from `tinymemory-core` with the rest of the agent tools — it is a `Tool`
// impl end to end, and the engine crate cannot name that trait.
#[cfg(feature = "memory")]
pub mod query;
/// Host-side re-ranking maths for the two agent search tools — a weighted
/// signal sum, cosine similarity, and an MMR pass. Above the driver by
/// design: see the module docs.
pub mod ranking;
#[cfg(feature = "memory")]
pub mod read_rpc;
#[cfg(feature = "memory")]
pub mod schemas;
/// The host-side per-turn memory-source allowlist. See the module docs for why
/// this is OpenHuman's and not the engine's.
pub mod source_scope;
#[cfg(feature = "memory")]
pub mod tools;

/// The disabled-build surface. See the gate note above.
#[cfg(not(feature = "memory"))]
mod stub;
#[cfg(not(feature = "memory"))]
pub use stub::*;

// Domains that are *mostly* extracted but keep their JSON-RPC surface here.
// Each of these is a thin wrapper: `pub use tinymemory_core::<domain>::*;`
// plus the handler/schema modules that name `RpcOutcome` and
// `ControllerSchema`. See the module docs on each for the split.
#[cfg(feature = "memory")]
pub mod conversations;
#[cfg(feature = "memory")]
pub mod diff;
#[cfg(feature = "memory")]
pub mod goals;
#[cfg(feature = "memory")]
pub mod people;
#[cfg(feature = "memory")]
pub mod schema;
#[cfg(feature = "memory")]
pub mod sources;
/// The golden-fixture seeder. Public so `tests/memory_golden_fixture_e2e.rs`
/// can drive it; it walks the RPC handlers, which is why it stayed host-side.
#[cfg(feature = "memory")]
pub mod store_golden;
#[cfg(feature = "memory")]
pub mod sync;
#[cfg(feature = "memory")]
pub mod tool_memory;
#[cfg(feature = "memory")]
pub mod tree;

#[cfg(all(test, feature = "memory"))]
mod api_identity_tests;
#[cfg(all(test, feature = "memory"))]
mod bypass_allowlist_tests;
#[cfg(all(test, feature = "memory"))]
mod direct_engine_refs_tests;
#[cfg(all(test, feature = "memory"))]
mod profile_conn_guard_tests;
#[cfg(all(test, feature = "memory"))]
mod seam_integration_tests;
#[cfg(all(test, feature = "memory"))]
mod sync_pipeline_e2e_tests;
#[cfg(all(test, feature = "memory"))]
mod tree_e2e_tests;

// ── The extracted subsystem, re-exported under its historical paths ─────────
//
// `pub use … as …` rather than `pub mod` — these are other crates' modules now.
// Every one of these was a `pub mod` here before the extraction.
// The engine's modules are **not** re-exported here any more.
//
// They used to be, under their historical `memory::…` paths, which made engine
// access indistinguishable from host-local code at every call site: a line
// reading `crate::openhuman::memory::store::chunks::…` never appeared in a
// `tinymemory_core` grep, so the audit that scoped this port undercounted the
// direct-engine surface roughly threefold. Every remaining caller now names
// `tinymemory_core::` explicitly, so `grep tinymemory_core` is an honest
// inventory of what still has to move behind the driver.

// Flat *type* re-exports, kept while the module facade above is gone.
//
// These are types, not module trees: `memory::MemoryCategory` names one value
// type, where `memory::store::…` opened the whole engine. They still have to
// move to `memory::api`'s equivalents, but they hide nothing in the meantime.
#[cfg(feature = "memory")]
pub use ops as rpc;
#[cfg(feature = "memory")]
pub use ops::*;
#[cfg(feature = "memory")]
pub use schemas::{
    all_controller_schemas as all_memory_controller_schemas,
    all_core_recall_controller_schemas as all_memory_core_recall_controller_schemas,
    all_core_recall_registered_controllers as all_memory_core_recall_registered_controllers,
    all_documents_controller_schemas as all_memory_documents_controller_schemas,
    all_documents_registered_controllers as all_memory_documents_registered_controllers,
    all_files_controller_schemas as all_memory_files_controller_schemas,
    all_files_registered_controllers as all_memory_files_registered_controllers,
    all_ingest_controller_schemas as all_memory_ingest_controller_schemas,
    all_ingest_registered_controllers as all_memory_ingest_registered_controllers,
    all_kv_graph_controller_schemas as all_memory_kv_graph_controller_schemas,
    all_kv_graph_registered_controllers as all_memory_kv_graph_registered_controllers,
    all_learn_controller_schemas as all_memory_learn_controller_schemas,
    all_learn_registered_controllers as all_memory_learn_registered_controllers,
    all_provider_controller_schemas as all_memory_provider_controller_schemas,
    all_provider_registered_controllers as all_memory_provider_registered_controllers,
    all_registered_controllers as all_memory_registered_controllers,
    all_sync_controller_schemas as all_memory_sync_controller_schemas,
    all_sync_registered_controllers as all_memory_sync_registered_controllers,
    all_tool_memory_controller_schemas as all_memory_tool_memory_controller_schemas,
    all_tool_memory_registered_controllers as all_memory_tool_memory_registered_controllers,
};
#[cfg(feature = "memory")]
pub use tinymemory_core::ingestion::{
    ExtractedEntity, ExtractedRelation, ExtractionMode, IngestionJob, IngestionQueue,
    IngestionState, IngestionStatusSnapshot, MemoryIngestionConfig, MemoryIngestionRequest,
    MemoryIngestionResult, DEFAULT_MEMORY_EXTRACTION_MODEL,
};
#[cfg(feature = "memory")]
pub use tinymemory_core::rpc_models::*;
// TYPE CARVE-OUT — named on the contract crate, not reached through the engine.
//
// `tinymemory_core::traits` is itself a `pub use` of exactly these items out of
// `tinymemory-api`, so the two spellings already resolve to ONE type; going
// through the engine to reach an engine-neutral contract is the thing the
// extraction exists to undo. Naming the contract directly is also what lets
// these six survive the `memory` gate: they appear in always-on agent-harness
// signatures, so a build with the family compiled out still needs them, and a
// stub twin would be a second definition free to drift.
pub use tinymemory_api::recall::RecallOpts;
pub use tinymemory_api::traits::Memory;
pub use tinymemory_api::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};

// Types that external tests and consumers historically imported from
// `memory::*`. The definitions moved to sibling crates during the memory
// refactor; these aliases keep the public surface stable.
#[cfg(feature = "memory")]
pub use tinymemory_core::store::types::NamespaceDocumentInput;
#[cfg(feature = "memory")]
pub use tinymemory_core::store::{MemoryClient, UnifiedMemory};
