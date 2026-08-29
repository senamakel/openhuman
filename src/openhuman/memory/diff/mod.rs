//! Host layer over [`tinymemory_core::diff`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::diff::…` path resolving.
//!
//! # What the glob actually carries, and what blocks removing it (#5560)
//!
//! Three things, and only one of them is engine-owned in a way that matters:
//!
//! - the eight serde wire types (`ChangeKind`, `Checkpoint`, `CrossSourceDiff`,
//!   `DiffResult`, `DiffSummary`, `ItemChange`, `Snapshot`, `SnapshotTrigger`)
//!   — these are `tinycortex::memory::diff::types`, which `rpc.rs` and
//!   `tools/diff.rs` already name directly, so they would repoint for free;
//! - `ops` (or its `memory-git`-off stub) — 312 lines of async wrappers over
//!   `tinycortex::memory::diff::DiffEngine`, reached from `rpc.rs` as
//!   `super::ops` and from the `memory_diff` agent tool;
//! - `source` — the `SnapshotItemSource` seam `ops` hands the engine.
//!
//! `ops` is what pins this. It reads the source registry
//! (`sources::registry::list_sources`, `sources::types::MemorySourceEntry`,
//! `sources::status::source_id_prefix`), so it cannot come home ahead of
//! `memory::sources` — see that module's docs for the two blockers there — and
//! `source` builds its snapshot items through
//! `store::chunks::store::with_connection`, a raw SQLite handle. Moving that
//! pair into the host would relocate the unpoliced door rather than close it.
//!
//! `MemoryDiff` is a real capability family — `capture_snapshot`, `snapshots`,
//! `diff` — and it is the right destination for three of `ops`' entry points.
//! It is not a drop-in for the module, for two reasons worth stating before
//! anyone tries:
//!
//! - **It covers three of `ops`' ten entry points.** `auto_snapshot_after_sync`,
//!   `diff_since_last`, `diff_since_read`, `mark_read`, `create_checkpoint`,
//!   `diff_since_checkpoint` and `cleanup` are checkpoints, read markers, the
//!   post-sync trigger and ledger retention; the family has no member for any
//!   of them, and `capture_snapshot` takes no `SnapshotTrigger`.
//! - **The wire shapes are different types.** The family answers `SnapshotRef`
//!   and `DiffReport`; `memory_diff.*` publishes `Snapshot`, `DiffResult` and
//!   `CrossSourceDiff`, which the Memory tab reads. Swapping the call without
//!   translating at the handler would change the RPC payloads, which is a
//!   behaviour change, not a migration.

// The eight wire types, named at the crate that defines them. They are
// `tinycortex::memory::diff::types` — `tinymemory_core::diff` re-exports them
// out of its `engine::backend::diff::types`, which is that same module — so
// this is the identical item under a different crate alias, and an explicit
// `use` shadows the glob below with itself. `rpc.rs` and `tools/diff.rs`
// already spell them this way; saying it here too means the flat
// `memory::diff::ChangeKind` paths keep resolving when `tinymemory-core`
// leaves the build, with no edit at any call site.
//
// Ungated on both sides of `memory-git`, mirroring tinycortex's own carve-out:
// these are serde-only values that always-on callers (the subconscious memory
// profile renders `CrossSourceDiff` and `ChangeKind` into prompts) name in a
// slim build too.
pub use tinycortex::memory::diff::types::{
    ChangeKind, Checkpoint, CrossSourceDiff, DiffResult, DiffSummary, ItemChange, Snapshot,
    SnapshotTrigger,
};

// What is left of the glob, and all that pins it: `ops` (or its `memory-git`-off
// stub) and `source`. Both are `tinymemory-core`'s own — see the header above
// for why neither can come home ahead of `memory::sources`, and why
// `MemoryDiff` covers three of `ops`' ten entry points rather than replacing it.
pub use tinymemory_core::diff::*;

#[cfg(feature = "memory-git")]
pub mod rpc;
#[cfg(feature = "memory-git")]
pub mod schemas;

#[cfg(feature = "memory-git")]
pub use schemas::{
    all_controller_schemas as all_memory_diff_controller_schemas,
    all_registered_controllers as all_memory_diff_registered_controllers,
};
// The agent tool came back with the rest of them; re-exported here so the
// historical `memory::diff::MemoryDiffTool` path keeps resolving.
#[cfg(feature = "memory-git")]
pub use crate::openhuman::memory::tools::diff::MemoryDiffTool;
#[cfg(not(feature = "memory-git"))]
mod stub;

#[cfg(not(feature = "memory-git"))]
pub use stub::{all_memory_diff_controller_schemas, all_memory_diff_registered_controllers};
