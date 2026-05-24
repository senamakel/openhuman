//! Topic-tree persistence — store, types, registry (from memory_tree::tree_topic).
//!
//! Only the persistence-only files are hosted here. Logic files
//! (backfill, curator, hotness, routing) remain in memory_tree::tree_topic.

pub mod registry;
pub mod store;
pub mod types;

pub use registry::{
    archive_topic_tree, force_create_topic_tree, get_or_create_topic_tree, list_topic_trees,
};
pub use store::{count, distinct_sources_for, get, get_or_fresh, upsert};
pub use types::{
    EntityIndexStats, HotnessCounters, TOPIC_ARCHIVE_THRESHOLD, TOPIC_CREATION_THRESHOLD,
    TOPIC_RECHECK_EVERY,
};
