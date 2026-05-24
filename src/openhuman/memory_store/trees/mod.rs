//! Generic tree persistence — store + types (previously at memory_tree::tree).
//!
//! Only the persistence-only files (store.rs, types.rs, store_tests.rs) are
//! hosted here. Tree logic (bucket_seal, flush, registry) stays in memory_tree.

pub mod store;
pub mod types;

pub use store::{get_summary_embedding, set_summary_embedding};
pub use types::{
    Buffer, SummaryNode, Tree, TreeKind, TreeStatus, INPUT_TOKEN_BUDGET, OUTPUT_TOKEN_BUDGET,
    SUMMARY_FANOUT,
};
