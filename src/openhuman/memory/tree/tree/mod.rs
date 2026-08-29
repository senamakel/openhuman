//! Host layer over [`tinymemory_core::tree::tree`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::tree::tree::…` path resolving.

// Engine-owned persistence types (`Tree`, `SummaryNode`, `Buffer`, `TreeKind`,
// the seal/flush/registry mechanics). None of them is a contract item, and the
// `TreeStatus` re-exported here is the engine's `store::trees` one — a
// different type from the contract's `TreeStatus`, which the sibling
// `tree_runtime` module exports. Two same-named types one module apart, so
// check which one a call site means before moving it.
//
// What keeps this line alive (#5560), so the next reader does not re-derive it:
// `read_rpc::admin`'s `flush_source_tree` needs `TreeFactory` and
// `flush::force_flush_tree`, and `integrations::composio::ops_tests` +
// `tests/raw_coverage/` reach `bucket_seal::{seal_one_level, LabelStrategy}`,
// `store::get_tree_by_scope`, `registry::is_unique_violation` and
// `set_summary_embedding`. `MemoryTree` on the contract is namespace-addressed
// (append / query_source / drill_down / seal / cascade / summary_forest /
// recent_leaves) and has no door onto a tree *object*, which is the ask.
pub use tinymemory_core::tree::tree::*;

pub mod rpc;
