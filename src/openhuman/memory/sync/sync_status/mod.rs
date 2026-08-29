//! Host layer over the engine's sync-status vocabulary.
//!
//! The domain itself lives in the engine; what stays here is its JSON-RPC
//! surface — handlers and controller schemas name OpenHuman's `RpcOutcome` and
//! `ControllerSchema`, which the engine crate cannot see. The re-export below
//! keeps every historical `memory::sync::sync_status::…` path resolving.
//!
//! # Why this names `tinycortex` rather than `tinymemory_core` (#5560)
//!
//! It used to read `pub use tinymemory_core::sync::sync_status::*;`, and that
//! glob had exactly two `pub` items — `FreshnessLabel` and `MemorySyncStatus`.
//! **Neither was the one this domain actually returns.** `rpc::status_list_rpc`
//! builds its reply from `tinycortex::memory::sync::{StatusListResponse,
//! MemorySyncStatus}`, produced by `tinycortex`'s SQLite-backed
//! `list_sync_statuses`; `tinymemory-core`'s pair is a second declaration of
//! the same two shapes with no producer behind it. Its own module doc says so:
//! "today the only *producer* is the engine's SQLite-backed
//! `list_sync_statuses`, which OpenHuman still calls directly … the serde shape
//! matches the engine's copy field for field."
//!
//! So this is not a substitute — it is the **same two items**, named through
//! the crate that defines them instead of through a twin. Verified rather than
//! assumed: both `FreshnessLabel`s are `#[serde(rename_all = "snake_case")]`
//! over `Active`/`Recent`/`Idle` with the identical `from_age_ms` thresholds
//! (30s → `Active`, 5min → `Recent`, else `Idle`), and both `MemorySyncStatus`
//! carry the same seven fields in the same order.
//!
//! That matters because `tinymemory-core` is what #5560 removes from the
//! production dependency graph, while `tinycortex` stays — it is a direct
//! dependency of this crate (`Cargo.toml`), it is where the engine actually
//! lives, and `rpc.rs` in this very directory already names
//! `tinycortex::memory::sync`. Same precedent as `memory::people`, which
//! repointed for the same reason.
//!
//! **Named, not globbed, and that is deliberate.** `tinycortex::memory::sync`
//! re-exports ~50 items — every sync pipeline, `SyncState`, `SyncDispatcher`,
//! the audit log — and pouring those into `memory::sync::sync_status::*` would
//! widen this path far past the two names the old glob carried. Two names in,
//! two names out.

pub use tinycortex::memory::sync::{FreshnessLabel, MemorySyncStatus};

pub mod rpc;
pub mod schemas;

// The controller aggregators this domain's RPC surface defines. Aliased
// exactly as the pre-extraction module exported them.
pub use schemas::{
    all_controller_schemas as all_memory_sync_status_controller_schemas,
    all_registered_controllers as all_memory_sync_status_registered_controllers,
};
