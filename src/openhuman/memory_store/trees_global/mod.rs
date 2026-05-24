//! Global-tree persistence (registry only) — moved from memory_tree::tree_global.
//!
//! Only the persistence-level registry.rs is hosted here. Logic files
//! (digest, recap, seal) remain in memory_tree::tree_global.
//! Note: tree_global had no standalone store.rs (uses tree::store directly).

pub mod registry;

pub use registry::get_or_create_global_tree;
