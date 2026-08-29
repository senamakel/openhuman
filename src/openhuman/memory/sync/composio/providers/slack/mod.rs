//! Host layer over [`tinymemory_core::sync::composio::providers::slack`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::sync::composio::providers::slack::…` path resolving.
//!
//! # This one looks droppable from `src/` alone, and is not (#5560)
//!
//! Nothing under `src/` names a single item the glob supplies: `rpc.rs` reaches
//! `SyncOutcome` through the parent `providers` module, and `schemas.rs` only
//! reaches `super::rpc`. Reading that as "unused" and deleting the line breaks
//! the build — four `tests/raw_coverage/` targets import `SlackProvider`,
//! `run_backfill_via_search` and `post_process` through **this** path, and the
//! engine's own module doc says `post_process` is `pub` for exactly that
//! reason.
//!
//! `cargo test --lib` does not compile `tests/`, so that break would land in
//! CI rather than locally. Grep both trees before narrowing any shim here.

pub use tinymemory_core::sync::composio::providers::slack::*;

pub mod rpc;
pub mod schemas;

// The controller aggregators this domain's RPC surface defines.
pub use schemas::*;
