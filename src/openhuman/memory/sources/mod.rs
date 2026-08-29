//! Host layer over [`tinymemory_core::sources`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::sources::…` path resolving.
//!
//! # Why this glob is still here (#5560), stated so nobody re-derives it
//!
//! On its face this looks like the easiest facade in the tree to bring home:
//! the registry it fronts is persisted in **this host's** `config.toml` under
//! `[[memory_sources]]`, and `tinymemory_core::sources` is mostly a wrapper —
//! `types` is `pub use tinymemory_sources::…`, `registry` wraps
//! `tinymemory_sources::registry::SourceRegistry` with the host's config path
//! and a write lock, and five of the seven readers (`folder`, `conversation`,
//! `rss`, `web_page`, `github`) delegate to `tinymemory_sources::readers`.
//! Three things stand between this file and that move, and all three are
//! outside it:
//!
//! 1. **The types have a home this crate does not depend on.** They are
//!    `tinymemory-sources`' — an engine-neutral crate the root manifest does
//!    not list. It is *reachable* without a new dependency: the `tinymemory`
//!    crate this manifest already takes re-exports it as `tinymemory::sources`
//!    behind an off-by-default `sources` feature (the network readers need
//!    `sources-network` on top). So the unlock is a feature line rather than a
//!    new entry — but it is still a `Cargo.toml` decision, not a code one.
//! 2. **Two of the seven readers have no upstream twin.** `composio` and
//!    `twitter` are implemented in `tinymemory-core` itself. Neither reaches
//!    engine internals — both name only `sources::types`, the host `Config`
//!    and the local `SourceReader` trait — so they are portable, but they are
//!    code to move rather than a re-export to repoint.
//! 3. **`sources::sync` and `sources::status` are wired into the pieces #5560
//!    has not moved yet.** `sync` reaches `engine::run_source_pipeline`,
//!    `engine::{needs_rebuild, rebuild_tree_from_raw}`,
//!    `queue::store::retry_all_failed`, `sync::composio` and `sync::audit`;
//!    `status` reaches `store::chunks::store::with_connection` — the ingest
//!    pipeline, the re-embed queue and the raw SQLite chunk door. (That last
//!    one is `status`', not `sync`'s; an earlier revision of this note
//!    attributed it to `sync`.) Porting the domain as it stands would move
//!    those into the host rather than behind the bus, which is the opposite of
//!    what the issue is for: a second unpoliced door spelled differently is
//!    still a second unpoliced door.
//!
//! ## What is *not* blocked, so nobody looks for it here
//!
//! The RPC layer above this glob no longer names the engine at all. [`rpc`]
//! moved its coding-session, raw-archive-coverage, audit-log and sync-price
//! calls onto `MemoryCodingSessions` / `MemorySourceSync` when tinymemory
//! v1.7.0 added them, so the two references left in this domain are the two in
//! *this file*. Neither is a call site anyone can reroute: they are the shim
//! itself.
//!
//! The unlock for them is a **manifest** decision, not a code one, and it has
//! not changed — `tinymemory` re-exports the types as `tinymemory::sources`
//! behind an off-by-default `sources` feature (`sources-network` on top for the
//! network readers) that the root `Cargo.toml` does not enable, and even with
//! it on, points 2 and 3 above still stand.
//!
//! So this stays until the queue and the sync pipeline go behind the module.
//! `MemorySourceSink` is not the answer either — it is `accept_source_items` +
//! `forget_source` + `forget_matching`, an *ingest* door, with no listing or
//! CRUD member for a configured connector. And it is the *whole* of
//! `Capability::Sources`: there is no `MemorySources` family in the contract,
//! so a listing member is an upstream ask, not a call site nobody routed.

pub use tinymemory_core::sources::*;

pub mod rpc;
pub mod schemas;

pub use tinymemory_core::sources::apply_kind_defaults;

// The controller aggregators this domain's RPC surface defines. Aliased
// exactly as the pre-extraction module exported them.
pub use schemas::{
    all_controller_schemas as all_memory_sources_controller_schemas,
    all_registered_controllers as all_memory_sources_registered_controllers,
};
