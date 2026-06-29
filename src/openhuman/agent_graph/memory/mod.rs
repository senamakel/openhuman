//! Memory hooks — pre-node context injection.
//!
//! Thin adapter over the existing [`MemoryLoader`] so a graph node can prime
//! long-term memory before an agent step, mirroring how the linear harness
//! injects memory per user message. Post-run distillation (the archivist) stays
//! on the existing [`PostTurnHook`](crate::openhuman::agent::hooks) surface —
//! graph runs that wrap an agent turn inherit it for free, so it is not
//! duplicated here.

use anyhow::Result;

use crate::openhuman::agent_memory::memory_loader::{DefaultMemoryLoader, MemoryLoader};
use crate::openhuman::memory::Memory;

/// Load relevant memory context for `query`, returning the formatted block the
/// caller can prepend to a node's prompt (empty string when nothing relevant).
pub async fn load_memory_context(memory: &dyn Memory, query: &str) -> Result<String> {
    tracing::debug!(
        query_chars = query.len(),
        "[agent_graph] loading memory context for graph node"
    );
    DefaultMemoryLoader::default()
        .load_context(memory, query)
        .await
}
