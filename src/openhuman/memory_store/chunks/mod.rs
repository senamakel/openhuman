//! SQLite-backed chunk storage — types and persistence layer.
//!
//! Previously at `memory::chunk_store` and `memory::chunk_types`.
//! Moved here as part of the memory_store consolidation.

pub mod store;
pub mod types;

// Re-export everything so `memory_store::chunks::*` works.
pub use store::*;
pub use types::*;
