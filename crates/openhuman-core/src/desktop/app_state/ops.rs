//! Core-owned `app_state` business logic: the polled snapshot RPC and local
//! on-disk state.
//!
//! Split by responsibility:
//! - [`types`] — the serde types exchanged with the frontend.
//! - [`state_file`] — atomic on-disk persistence of `StoredAppState`.
//! - [`runtime_snapshot`] — the local-AI + service half of the snapshot.
//! - [`snapshot`] — the `app_state_snapshot` / `update_local_state` RPC handlers.
//!
//! The current user is no longer refreshed here: the host that owns the
//! session (the desktop shell's `openhuman-session`) caches `/auth/me`, and
//! the snapshot reports the payload the host handed the core at login.

mod runtime_snapshot;
mod snapshot;
mod state_file;
mod types;

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;

// Test-only flat re-import of every submodule's internals, so the test file
// above — written against this module's pre-split flat scope — keeps working
// via `use super::*;`.
#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
use runtime_snapshot::*;
#[cfg(test)]
use snapshot::sanitize_snapshot_user;
#[cfg(test)]
use state_file::*;

/// Shared log prefix for every `[app_state]`-tagged debug/warn line across
/// these submodules.
pub(super) const LOG_PREFIX: &str = "[app_state]";

pub use snapshot::{snapshot, update_local_state};
pub(crate) use state_file::load_stored_app_state;
pub use state_file::save_app_state;
pub use types::{
    AppStateSnapshot, RuntimeSnapshot, StoredAppState, StoredAppStatePatch, StoredOnboardingTasks,
};
