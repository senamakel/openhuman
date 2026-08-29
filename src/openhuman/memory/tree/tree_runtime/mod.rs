//! Host layer over [`tinymemory_core::tree::tree_runtime`].
//!
//! The domain itself lives in the extracted crate; what stays here is its
//! JSON-RPC surface — handlers and controller schemas name OpenHuman's
//! `RpcOutcome` and `ControllerSchema`, which the engine crate cannot see.
//! The glob re-export keeps every historical `memory::tree::tree_runtime::…` path resolving.
//!
//! # What is left of the glob, and the one thing that pins it (#5560)
//!
//! Everything below this line is already off the engine crate: the node model
//! is named on the contract (see the next comment), and `ops.rs` reaches the
//! summarisation entry points at `tinycortex::memory::tree::runtime::*`
//! directly. What the glob still supplies is exactly two modules:
//!
//! - `engine` — `run_summarization` / `rebuild_tree` / `run_hourly_loop`,
//!   thin adapters that wrap the host `Config` around tinycortex's `*_observed`
//!   entry points and publish `TreeSummarizer*` events.
//! - `store` — ~18 `Config`-taking wrappers over
//!   `tinycortex::memory::tree::runtime::store`.
//!
//! Both are one step: they call `engine_config(config)`, which is
//! `tinymemory_core::tinycortex::memory_config_from`. So the blocker here is
//! not a capability family, it is that **nothing host-side can build a
//! `tinycortex::memory::MemoryConfig`.** That function resolves the embedder
//! slug through the engine's own resolution ladder (`effective_embedder_slug`),
//! and keying a vector sidecar on the wrong provider files local vectors under
//! the cloud one — so it is not a helper worth reimplementing from the outside.
//! `memory::sync::sync_status::rpc` and `memory::tools::flavour` are blocked on
//! the same call.

pub use tinymemory_core::tree::tree_runtime::*;

// The summary-tree node model is **contract** vocabulary, not engine
// vocabulary: it is defined in `tinymemory-bus` and the engine crate re-exports
// the same items, so these names and the ones the glob above would supply are
// one set of types, not two. Naming the contract explicitly (an explicit `use`
// shadows a glob, and here it shadows it with itself) records which half of
// this shim survives the engine leaving the build — and means the call sites on
// `memory::tree::tree_runtime::estimate_tokens` and friends need no edit when
// it does.
pub use crate::openhuman::memory::api::tree as types;
pub use crate::openhuman::memory::api::tree::{
    derive_node_ids, derive_parent_id, estimate_tokens, level_from_node_id, node_id_to_path,
    IngestRequest, NodeLevel, QueryResult, TreeNode, TreeStatus,
};

pub mod ops;
pub mod schemas;

pub use ops as rpc;

pub mod bus;

/// The `openhuman memory tree` CLI subcommands, which drive the RPC handlers.
pub mod cli;

// The controller aggregators this domain's RPC surface defines. Aliased
// exactly as the pre-extraction module exported them.
pub use schemas::{
    all_controller_schemas as all_tree_summarizer_controller_schemas,
    all_registered_controllers as all_tree_summarizer_registered_controllers,
};
