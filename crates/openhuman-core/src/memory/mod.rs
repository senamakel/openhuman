//! Memory: TinyMemory's agent lifecycle, bound to OpenHuman
//! (`docs/specs/memory-v2.md`; contract and lifecycle in `vendor/tinymemory`).
//!
//! | Module | Role |
//! | --- | --- |
//! | [`engine`] | Maps `[memory]` configuration and host credentials into TinyMemory's native engine binder |
//! | native engine binder | Scrubs writes under the host policy and times every operation |
//! | [`scope`] | Config-to-identity pins and the account layout migration hooks |
//! | [`lifecycle`] | The turn hooks (pre-turn pack, post-turn log, compaction recall) and the background job queue |
//! | [`ops`] | Engine selection, recall, fetch, learn, forget, list, the pack preview |
//! | [`brain`] | The shared brain: documents by source type |
//! | [`sources`] | The `[[memory.sources]]` registry and sync into the brain (folder, file, link, github, rss, composio) |
//! | [`backfill`] | Consent-gated storing of chats from before turns were logged |
//! | [`import`] | Consent-gated, resumable import of a v1 store |
//! | [`tools`] | The single `memory` agent tool |
//! | [`bus`] | The cron subscriber (source sync, background jobs) |
//! | [`schemas`] | The `openhuman.memory_*` controllers |
//!
//! Chat thread persistence is not memory: it lives in [`crate::threads::store`].

pub mod backfill;
pub mod billing;
pub mod brain;
pub mod bus;
pub mod deletion;
pub mod engine;
pub mod error;
pub mod explore;
pub(crate) mod files;
#[cfg(test)]
mod guard {
    use std::sync::Arc;

    use tinymemory_api::{MemoryEngine, StoreItem};
    use tinymemory_integrations::safety::{scrub_item_with, Policy};
    use tinymemory_integrations::EngineBinder;

    fn wrap(engine: Arc<dyn MemoryEngine>) -> Arc<dyn MemoryEngine> {
        EngineBinder::default()
            .bind_external(engine, "test://engine", Policy::corroborated())
            .engine
    }

    fn scrub(item: StoreItem) -> StoreItem {
        scrub_item_with(item, Policy::corroborated()).value
    }

    mod tests {
        use async_trait::async_trait;
        use tinymemory_api::*;

        include!("guard_tests.rs");
    }
}
pub mod import;
pub mod layout_migration;
pub mod lifecycle;
pub mod local_root;
pub mod ops;
pub mod schemas;
pub mod scope;
pub mod sources;
pub mod status;
mod tool_budget;
pub(crate) mod tool_writes;
pub mod tools;
pub mod types;

#[cfg(test)]
pub(crate) mod test_fixtures;

#[cfg(test)]
#[path = "confinement_tests.rs"]
mod confinement_tests;

pub use bus::register_memory_subscribers;
pub use engine::is_on as memory_is_on;
pub use schemas::{
    all_controller_schemas as all_memory_controller_schemas,
    all_registered_controllers as all_memory_registered_controllers,
};
pub use tools::{MemoryTool, MEMORY_TOOL_NAME};
