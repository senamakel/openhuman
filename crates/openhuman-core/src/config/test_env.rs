//! Shared process-env guard for the crate's unit tests.
//!
//! Process env is global, so every test that mutates it serializes on
//! [`crate::config::TEST_ENV_LOCK`] and puts the previous values back when it
//! finishes, even on panic. [`EnvVarGuard`] is the one RAII type for that:
//!
//! ```ignore
//! // lock + one var, restored and unlocked on drop
//! let _env = EnvVarGuard::locked_set("OPENHUMAN_X", "1");
//! // lock + several vars
//! let _env = EnvVarGuard::locked().with("A", "1").without("B");
//! // the test already holds the lock itself (or a stricter one)
//! let _env = EnvVarGuard::set("A", "1");
//! ```
//!
//! Lock order across the crate is `module_guard` -> cache lock ->
//! `TEST_ENV_LOCK`: take this guard last. The lock is released only after
//! every variable has been restored (field order below), so the next test
//! never observes a half-restored environment.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::MutexGuard;

use super::TEST_ENV_LOCK;

/// The workspace override most tests pin.
pub(crate) const WORKSPACE: &str = "OPENHUMAN_WORKSPACE";

/// Take the shared env lock, recovering from poison so one panicking test
/// cannot wedge the rest of the binary.
pub(crate) fn lock_env() -> MutexGuard<'static, ()> {
    TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Sets or removes env vars and restores each previous value on drop, in
/// reverse order. Optionally owns the [`TEST_ENV_LOCK`] guard for its lifetime.
pub(crate) struct EnvVarGuard {
    // Declared before the lock: `Drop::drop` restores the vars, then the
    // fields drop in order, so the lock is released last.
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: Option<MutexGuard<'static, ()>>,
}

impl EnvVarGuard {
    /// Empty guard that holds nothing; add vars with [`Self::with`] /
    /// [`Self::without`]. The caller must already hold the env lock.
    pub(crate) fn new() -> Self {
        Self {
            saved: Vec::new(),
            _lock: None,
        }
    }

    /// Empty guard that takes [`TEST_ENV_LOCK`] and holds it until dropped.
    pub(crate) fn locked() -> Self {
        Self {
            saved: Vec::new(),
            _lock: Some(lock_env()),
        }
    }

    /// Set `key` (caller holds the env lock).
    pub(crate) fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        Self::new().with(key, value)
    }

    /// Remove `key` (caller holds the env lock).
    pub(crate) fn unset(key: &'static str) -> Self {
        Self::new().without(key)
    }

    /// Remove every key in `keys` (caller holds the env lock).
    pub(crate) fn unset_many(keys: &[&'static str]) -> Self {
        keys.iter()
            .fold(Self::new(), |guard, key| guard.without(key))
    }

    /// Pin `OPENHUMAN_WORKSPACE` to `path` (caller holds the env lock).
    pub(crate) fn workspace_unlocked(path: impl AsRef<Path>) -> Self {
        Self::set(WORKSPACE, path.as_ref())
    }

    /// Take the env lock, then set `key`.
    pub(crate) fn locked_set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        Self::locked().with(key, value)
    }

    /// Take the env lock, then remove `key`.
    pub(crate) fn locked_unset(key: &'static str) -> Self {
        Self::locked().without(key)
    }

    /// Take the env lock, then remove every key in `keys`.
    pub(crate) fn locked_unset_many(keys: &[&'static str]) -> Self {
        keys.iter()
            .fold(Self::locked(), |guard, key| guard.without(key))
    }

    /// Take the env lock, then pin `OPENHUMAN_WORKSPACE` to `path`.
    pub(crate) fn workspace(path: impl AsRef<Path>) -> Self {
        Self::locked_set(WORKSPACE, path.as_ref())
    }

    /// Also set `key` for this guard's lifetime.
    pub(crate) fn with(mut self, key: &'static str, value: impl AsRef<OsStr>) -> Self {
        self.saved.push((key, std::env::var_os(key)));
        std::env::set_var(key, value.as_ref());
        self
    }

    /// Also remove `key` for this guard's lifetime.
    pub(crate) fn without(mut self, key: &'static str) -> Self {
        self.saved.push((key, std::env::var_os(key)));
        std::env::remove_var(key);
        self
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (key, old) in self.saved.drain(..).rev() {
            match old {
                // `OPENHUMAN_WORKSPACE` has no meaningful ambient value under
                // test: a prior value can only be another test's deliberate
                // leak (e.g. the detached-task composio bus tests keep their
                // workspace pointed at a leaked dir). Restoring it would
                // carry that stale path into unrelated tests, so clear it,
                // as the per-file guards this replaced did.
                Some(_) if key == WORKSPACE => std::env::remove_var(key),
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
