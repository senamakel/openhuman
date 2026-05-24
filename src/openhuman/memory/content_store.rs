//! Re-export shim — implementation has moved to `memory_store::content`.
//!
//! This module exists only for backwards compatibility while the import sweep
//! (step 6) is pending. Do not add new code here.

pub use crate::openhuman::memory_store::content::*;

// Re-export submodules so `memory::content_store::atomic`, etc. still work.
pub use crate::openhuman::memory_store::content::atomic;
pub use crate::openhuman::memory_store::content::compose;
pub use crate::openhuman::memory_store::content::obsidian;
pub use crate::openhuman::memory_store::content::paths;
pub use crate::openhuman::memory_store::content::raw;
pub use crate::openhuman::memory_store::content::read;
pub use crate::openhuman::memory_store::content::tags;
