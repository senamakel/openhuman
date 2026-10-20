//! Moving memory from the legacy CortexDB tree into the per-user tree.
//!
//! TinyMemory owns the migration job and service. These modules bind the host
//! ports, file-backed state, and RPC/background scheduler.

pub mod app_host;
pub mod ops;
pub mod state;

#[cfg(test)]
pub(crate) mod test_host;

pub use app_host::AppHost;
pub use ops::{retry, scan, start, status, tick, StartParams};
pub use state::FileStateStore;
pub use tinymemory_tools::layout_migration::{
    LayoutHost, MigrationState, MigrationStateStore, MigrationStatus, Outcome, Phase, ScanView,
    Trigger,
};
