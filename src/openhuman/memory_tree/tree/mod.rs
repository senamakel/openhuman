//! Generic summary-tree mechanics shared by all three tree flavors:
//! Source (per ingest source), Global (cross-source digest), and Topic
//! (per-entity). Covers storage, buffer management, bucket-seal cascade,
//! time-based flush, and the get-or-create registry primitive.
//!
//! Source-specific policy (the `_source.md` on-disk mirror, the
//! `get_or_create_source_tree` wrapper) lives in the sibling
//! [`crate::openhuman::memory_tree::sources`] module.
//!
//! Global and topic policies (scope constants, hotness gates, curator)
//! live in [`crate::openhuman::memory_tree::tree_global`] and
//! [`crate::openhuman::memory_tree::tree_topic`] respectively; both
//! import generic primitives from this module.

pub mod bucket_seal;
pub mod flush;
pub mod registry;
pub mod store;
pub mod types;

pub use bucket_seal::{append_leaf, append_leaf_deferred, LabelStrategy, LeafRef};
pub use registry::{get_or_create_tree, new_summary_id, new_tree_id};
pub use store::{get_summary_embedding, set_summary_embedding};
pub use types::{
    Buffer, SummaryNode, Tree, TreeKind, TreeStatus, INPUT_TOKEN_BUDGET, OUTPUT_TOKEN_BUDGET,
    SUMMARY_FANOUT,
};
