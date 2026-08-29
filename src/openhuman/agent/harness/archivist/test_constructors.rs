//! Test-only constructors for `ArchivistHook` that inject stub providers
//! directly, bypassing `with_config`'s provider-build logic.
//!
//! This module is `#[cfg(test)]` at its declaration in `archivist/mod.rs`, so
//! the engine-crate import below is not linked by a production build — the
//! engine is a dev-dependency for exactly this kind of fixture (#5560). The
//! `ChatProvider` it injects is an override for the memory summariser's own
//! chat task-local, not a handle the archivist calls; see
//! `types::ArchivistHook::chat_provider`.

use super::boundary::BoundaryConfig;
use super::types::ArchivistHook;
use crate::openhuman::config::Config;
use crate::openhuman::memory::api::provider::MemoryProvider;
use std::sync::Arc;
use tinymemory_core::chat::ChatProvider;

#[cfg(test)]
impl ArchivistHook {
    /// Test-only constructor that injects a stub `ChatProvider` directly,
    /// bypassing `with_config`'s provider-build logic. Used by Phase 1 tests to
    /// verify LLM recap and embedding paths without hitting a real LLM or Ollama
    /// daemon. Embedding is driven through `provider.as_scoring()` at call time.
    /// Exposed as `pub(crate)` so Phase 3 STM recall integration tests can drive
    /// the full archivist path.
    pub(crate) fn new_with_stubs(
        provider: Arc<dyn MemoryProvider>,
        chat_provider: Arc<dyn ChatProvider>,
    ) -> Self {
        Self {
            provider: Some(provider),
            enabled: true,
            boundary_config: BoundaryConfig::default(),
            config: Some(Config::default()),
            // Injecting a stub is the assertion that the LLM path runs, so the
            // availability flag `with_config` would have probed is set here.
            summariser_available: true,
            chat_provider: Some(chat_provider),
        }
    }

    /// Test-only constructor that injects stub providers AND a `Config`, so the
    /// Phase 2 segment-tree ingest path (gated by
    /// `config.learning.chat_to_tree_enabled`) can be exercised hermetically.
    ///
    /// `config.learning.chat_to_tree_enabled` must be set to `true` by the caller
    /// for the tree ingest to fire; the hook does NOT force it on.
    pub(crate) fn new_with_stubs_and_config(
        provider: Arc<dyn MemoryProvider>,
        chat_provider: Arc<dyn ChatProvider>,
        config: Config,
    ) -> Self {
        Self {
            provider: Some(provider),
            enabled: true,
            boundary_config: BoundaryConfig::default(),
            config: Some(config),
            summariser_available: true,
            chat_provider: Some(chat_provider),
        }
    }
}
