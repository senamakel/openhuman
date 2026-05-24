//! Topic tree policy over the generic tree engine.
//!
//! This is now a thin module root that gathers the remaining topic-specific
//! algorithms while avoiding a dedicated `tree/topic/` directory.

#[path = "topic_backfill.rs"]
pub mod backfill;
#[path = "topic_curator.rs"]
pub mod curator;
#[path = "topic_hotness.rs"]
pub mod hotness;
#[path = "topic_routing.rs"]
pub mod routing;

use crate::openhuman::memory_tree::tree::TreeFactory;

pub use crate::openhuman::memory_store::trees::hotness as store;
pub use crate::openhuman::memory_store::trees::registry;
pub use crate::openhuman::memory_store::trees::types;
pub use crate::openhuman::memory_store::trees::{
    archive_topic_tree, force_create_topic_tree, get_or_create_topic_tree, list_topic_trees,
};
pub use crate::openhuman::memory_store::trees::{
    EntityIndexStats, HotnessCounters, TOPIC_ARCHIVE_THRESHOLD, TOPIC_CREATION_THRESHOLD,
    TOPIC_RECHECK_EVERY,
};
pub use curator::{maybe_spawn_topic_tree, SpawnOutcome};
pub use hotness::{hotness, recency_decay};
pub use routing::route_leaf_to_topic_trees;

/// Canonical factory for one topic tree scope / entity id.
pub fn factory(scope: &str) -> TreeFactory<'_> {
    TreeFactory::topic(scope)
}
