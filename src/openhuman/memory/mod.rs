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
//! What is left below is a handful of flat **type** re-exports, kept so the
//! ~550 `crate::openhuman::memory::…` paths elsewhere in this crate keep
//! resolving unchanged. The engine's *modules* are no longer re-exported here
//! at all — see the block at the bottom of this file for why that facade was
//! deleted.
//!
//! **Prefer `tinymemory_api::…` in new code, never `tinymemory_core::…`.** This
//! paragraph used to say the opposite, which predates #5560: the contract crate
//! is what the host, `ModuleMemoryProvider` and the loaded module all speak, and
//! naming the engine crate is the thing that keeps it linked into the shipped
//! binary. A path that has no contract equivalent is a gap to report, not a
//! reason to reach for the engine.

pub mod agent;
pub mod api;
pub mod binding;
pub mod driver;
pub mod guard;
pub mod host;
/// Host implementations of the seam traits the ENGINE declares.
///
/// `#[cfg(test)]` because the production host embeds no engine any more: the
/// module installs its own seams and this host answers it over the bus through
/// [`crate::openhuman::modules::memory_host`] instead. What still needs these
/// is the test suite, which binds the in-process TinyCortex driver directly —
/// legitimate, because `tinymemory-core` is a dev-dependency there and a
/// dev-dependency is not linked into the shipped binary.
pub mod host_impls;
/// Host desktop policy: is the memory content root a vault Obsidian already
/// knows about? See the module docs for why this is OpenHuman's and not the
/// engine's.
pub mod obsidian_registry;
pub mod ops;
pub mod preferences;
pub mod sync_events_bridge;
// The consolidated `memory_query` agent tool and its six retrieval modes. Came
// back from `tinymemory-core` with the rest of the agent tools — it is a `Tool`
// impl end to end, and the engine crate cannot name that trait.
pub mod query;
pub mod read_rpc;
/// The host-side secret / PII scrubbers applied to anything this host persists
/// or hands on. See the module docs for why this is OpenHuman's and not the
/// engine's.
pub mod safety;
pub mod schemas;
/// The host-side per-turn memory-source allowlist. See the module docs for why
/// this is OpenHuman's and not the engine's.
pub mod source_scope;
#[cfg(test)]
pub(crate) mod test_support;
pub mod tools;

// Domains that are *mostly* extracted but keep their JSON-RPC surface here.
// Each of these is a thin wrapper: `pub use tinymemory_core::<domain>::*;`
// plus the handler/schema modules that name `RpcOutcome` and
// `ControllerSchema`. See the module docs on each for the split.
pub mod conversations;
pub mod diff;
pub mod goals;
pub mod people;
pub mod schema;
pub mod sources;
pub mod sync;
pub mod tool_memory;
pub mod tree;

#[cfg(test)]
mod api_identity_tests;
#[cfg(test)]
mod bypass_allowlist_tests;
#[cfg(test)]
mod direct_engine_refs_tests;
#[cfg(test)]
mod profile_conn_guard_tests;
#[cfg(test)]
mod seam_integration_tests;
#[cfg(test)]
mod sync_pipeline_e2e_tests;
#[cfg(test)]
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
pub use ops as rpc;
pub use ops::*;
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
// The ingestion vocabulary, split by who actually defines it (#5560).
//
// It was one `pub use tinymemory_core::ingestion::{…}` line of eleven names,
// which read as eleven engine types. Seven of them are not: `tinymemory-core`'s
// `ingestion` module re-exports them out of `engine::backend::ingest`, which is
// `pub use tinycortex::memory::ingest`. So the line below names the **same
// items** at the crate that defines them — `tinycortex` stays a direct
// dependency of this crate, `tinymemory-core` is the one being shed — and no
// type, wire byte or call site changes. Same move as `memory::people`,
// `memory::tool_memory` and `memory::tree::health`'s taxonomy half.
pub use tinycortex::memory::ingest::{
    ExtractedEntity, ExtractedRelation, ExtractionMode, MemoryIngestionConfig,
    MemoryIngestionRequest, MemoryIngestionResult, DEFAULT_MEMORY_EXTRACTION_MODEL,
};
// The remaining four are genuinely `tinymemory-core`'s own: the in-process
// ingest queue (`ingestion::queue`) and its status snapshot
// (`ingestion::state`). They have no contract equivalent — the ingest queue is
// named in `direct_engine_refs_tests`' upstream-gap list as one of the four
// things with no bus representation at all — so this is what still pins the
// engine crate here, and it is now visible as such rather than hidden in a
// list of eleven.
//
// `IngestionState` is the one with a live consumer
// (`tests/raw_coverage/memory_raw_coverage_e2e.rs`). The other three are kept
// because they are one module's worth of a single domain and splitting a queue
// from its own job type would leave a re-export that documents nothing; when
// the queue moves behind the bus they go together.
pub use tinymemory_core::ingestion::{
    IngestionJob, IngestionQueue, IngestionState, IngestionStatusSnapshot,
};
// The host's own JSON-RPC request/response shapes. They lived in
// `tinymemory_core::rpc_models` and were re-exported here by a glob; nothing
// in `tinymemory` ever named one, so the engine crate was carrying this host's
// RPC surface (#5560). Same glob, same paths, same wire bytes — the definitions
// are simply ours now. See `rpc_models`'s module docs.
pub mod rpc_models;
pub use rpc_models::*;
// Named on the crate directly — `traits` is engine scaffolding, not bus
// vocabulary, and is deliberately not re-exported from `memory::api` (#5560).
pub use crate::openhuman::memory::api::types::{
    MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary, RecallOpts,
};
pub use tinymemory_api::traits::Memory;

// Types that external tests and consumers historically imported from
// `memory::*`. The definitions moved to sibling crates during the memory
// refactor; these aliases keep the public surface stable.
//
// `MemoryClient` and `UnifiedMemory` are gone from this list: both were engine
// handles re-exported for consumers that no longer exist (grep found none in
// `src/`, `tests/` or the shell), and an unused alias is a compile-time link to
// the engine bought for nobody (#5560). A caller that genuinely needs the
// in-process handle names the crate deliberately rather than reaching it
// through the memory module's public surface.
pub use crate::openhuman::memory::api::types::NamespaceDocumentInput;
