//! Core-owned `app_state` business logic: the polled snapshot RPC, local
//! on-disk state, and the current-user cache that backs both.
//!
//! Split by responsibility:
//! - [`types`] — the serde types exchanged with the frontend.
//! - [`state_file`] — atomic on-disk persistence of `StoredAppState`.
//! - [`auth_timeout`] — the `auth_get_me` fetch timeout and its derived backoff base.
//! - [`current_user_fetch`] — the plain `GET /auth/me` HTTP call.
//! - [`current_user`] — the positive/negative current-user caches and their
//!   blocking/background refresh paths.
//! - [`current_user_generation`] — sign-out invalidation of those caches.
//! - [`staleness`] — how old the data a snapshot is serving has become.
//! - [`pending_session`] — activating or rejecting a not-yet-confirmed session.
//! - [`runtime_snapshot`] — the local-AI + service half of the snapshot.
//! - [`snapshot`] — the `app_state_snapshot` / `update_local_state` RPC handlers.

mod auth_timeout;
mod current_user;
mod current_user_fetch;
mod current_user_generation;
mod pending_session;
mod runtime_snapshot;
mod snapshot;
mod staleness;
mod state_file;
mod types;

#[cfg(test)]
#[path = "ops_current_user_backoff_tests.rs"]
mod current_user_backoff_tests;
#[cfg(test)]
#[path = "ops_snapshot_latency_tests.rs"]
mod snapshot_latency_tests;
#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;

// Test-only flat re-import of every submodule's internals, so the test
// files below — written against this module's pre-split flat scope — keep
// working unmodified via `use super::*;`. Each submodule marks the items
// these tests reach into as `pub(super)` (visible to `ops` and, therefore,
// to these test descendants) for exactly this purpose.
//
// External types the tests reach through `super::*` (rather than importing
// themselves) also need to be back in this flat scope, since a plain `use`
// inside a submodule is private to it and is not picked up by a glob import
// of that submodule.
#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use crate::security::credentials::{AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME};
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use std::collections::{BTreeMap, HashMap};
#[cfg(test)]
use std::time::{Duration, Instant};

#[cfg(test)]
use auth_timeout::*;
#[cfg(test)]
use current_user::*;
#[cfg(test)]
use current_user_fetch::*;
#[cfg(test)]
use current_user_generation::*;
#[cfg(test)]
use pending_session::*;
#[cfg(test)]
use runtime_snapshot::*;
#[cfg(test)]
use staleness::*;
#[cfg(test)]
use state_file::*;

/// Shared log prefix for every `[app_state]`-tagged debug/warn line across
/// these submodules.
pub(super) const LOG_PREFIX: &str = "[app_state]";

pub use auth_timeout::{
    parse_auth_fetch_timeout_secs, AUTH_FETCH_TIMEOUT_ENV_VAR, DEFAULT_AUTH_FETCH_TIMEOUT_SECS,
    MAX_AUTH_FETCH_TIMEOUT_SECS, MIN_AUTH_FETCH_TIMEOUT_SECS,
};
pub use current_user::peek_cached_current_user_identity;
pub use current_user_generation::{forget_current_user_caches, CURRENT_USER_SESSION_MUTATION_LOCK};
pub use snapshot::{snapshot, update_local_state};
pub(crate) use state_file::load_stored_app_state;
pub use state_file::save_app_state;
pub use types::{
    AppStateSnapshot, RuntimeSnapshot, StoredAppState, StoredAppStatePatch, StoredOnboardingTasks,
};
