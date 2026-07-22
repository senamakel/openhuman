use std::sync::Arc;

use crate::openhuman::memory::Memory;

pub use tinycortex::memory::tool_memory::store::{ToolMemoryStore, TOOL_MEMORY_PROMPT_CAP};

/// Build the crate-owned store over OpenHuman's shared memory object.
pub fn tool_memory_store(memory: Arc<dyn Memory>) -> ToolMemoryStore {
    ToolMemoryStore::new(memory)
}
