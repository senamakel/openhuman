//! Domain types for the tool-scoped memory layer — thin host re-export of
//! `tinycortex::memory::tool_memory::types`.
//!
//! This module preserves the public `memory_tools::types::*` import path while
//! TinyCortex remains the single implementation and wire-format authority.

pub use tinycortex::memory::tool_memory::types::{
    tool_memory_namespace, ToolMemoryPriority, ToolMemoryRule, ToolMemorySource,
};
