//! Compatibility shim: composio sync subscribers now live in
//! `memory_sync::composio::bus`.

#[cfg(feature = "memory")]
pub use crate::openhuman::memory::sync::composio::bus::*;
