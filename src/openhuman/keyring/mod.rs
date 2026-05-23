//! OS-keychain backed secret storage with a dev-mode file backend.
//!
//! Wraps the [`keyring`] crate (or a file-based backend in dev) to provide a
//! namespaced, user-scoped interface to secret storage:
//! - **macOS**: Keychain (prod)
//! - **Windows**: Credential Manager (prod)
//! - **Linux**: Secret Service / libsecret (prod)
//! - **Any platform — dev**: JSON file at `{workspace}/dev-keychain.json`
//!
//! All keys are scoped under a `user_id` parameter so multiple users can
//! coexist without collision.  The backend entry key format is:
//! `"{user_id}:{logical_key}"`.
//!
//! # Backend selection
//!
//! The backend is chosen **once** at first use, in this priority order:
//!
//! 1. `OPENHUMAN_KEYRING_BACKEND` env var: `"os"` | `"file"` | `"mock"`.
//! 2. `cfg!(debug_assertions)` → `file`.
//! 3. `OPENHUMAN_APP_ENV` == `"dev"` or `"staging"` → `file`.
//! 4. Otherwise → `os` (production).
//!
//! The selected backend is logged once with `[keyring] backend=<name> ...`.
//!
//! # Linux headless note
//!
//! On servers or CI without a Secret Service daemon, [`is_available`] returns
//! `false` when the `os` backend is selected.  Callers that opt out of keychain
//! storage (file-encrypted JSON fallback) check this flag.  The `file` backend
//! always reports as available.

pub mod error;
pub mod backend;

#[cfg(test)]
mod tests;

pub use error::KeyringError;
pub use backend::KeyringBackend;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chacha20poly1305::aead::{OsRng, rand_core::RngCore};

// ── Global state ─────────────────────────────────────────────────────────────

/// The workspace directory provided by the caller at startup.
///
/// Used by [`FileBackend`] to locate `dev-keychain.json`.  If not set, falls
/// back to the same env-var derivation as the config subsystem.
static WORKSPACE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// The selected backend, initialized on first use.
static BACKEND: OnceLock<Box<dyn KeyringBackend>> = OnceLock::new();

// ── Initialization ────────────────────────────────────────────────────────────

/// Register the workspace directory for the `file` backend.
///
/// Call this once at application startup (before any keyring operation) so the
/// `FileBackend` knows where to write `dev-keychain.json`.  If not called, the
/// backend derives a default path from env vars.
pub fn init_workspace(workspace_dir: &Path) {
    if WORKSPACE_DIR.set(workspace_dir.to_path_buf()).is_err() {
        // Already initialized — harmless, but log at debug to aid diagnostics.
        log::debug!("[keyring] init_workspace called after initialization; ignored");
    }
}

/// Returns the selected backend, initializing it on first call.
fn backend() -> &'static dyn KeyringBackend {
    BACKEND.get_or_init(|| {
        let b = build_backend();
        b
    }).as_ref()
}

fn build_backend() -> Box<dyn KeyringBackend> {
    // Priority 1: explicit env var override.
    if let Ok(env_val) = std::env::var("OPENHUMAN_KEYRING_BACKEND") {
        match env_val.trim() {
            "os" => {
                log::info!("[keyring] backend=os (OPENHUMAN_KEYRING_BACKEND override)");
                return Box::new(backend::OsBackend);
            }
            "file" => {
                let path = workspace_dir_for_file_backend();
                log::info!(
                    "[keyring] backend=file path={} (OPENHUMAN_KEYRING_BACKEND override)",
                    path.display()
                );
                return Box::new(backend::FileBackend::new(&path));
            }
            other => {
                log::warn!(
                    "[keyring] unknown OPENHUMAN_KEYRING_BACKEND={other:?}; falling through to defaults"
                );
            }
        }
    }

    // Priority 2: debug build → file backend.
    if cfg!(debug_assertions) {
        let path = workspace_dir_for_file_backend();
        log::info!(
            "[keyring] backend=file path={} (debug_assertions build)",
            path.display()
        );
        return Box::new(backend::FileBackend::new(&path));
    }

    // Priority 3: OPENHUMAN_APP_ENV dev/staging → file backend.
    if let Ok(app_env) = std::env::var("OPENHUMAN_APP_ENV") {
        match app_env.trim() {
            "dev" | "staging" => {
                let path = workspace_dir_for_file_backend();
                log::info!(
                    "[keyring] backend=file path={} (OPENHUMAN_APP_ENV={})",
                    path.display(),
                    app_env.trim()
                );
                return Box::new(backend::FileBackend::new(&path));
            }
            _ => {}
        }
    }

    // Priority 4: production OS backend.
    log::info!("[keyring] backend=os");
    Box::new(backend::OsBackend)
}

/// Derive the workspace directory for the `FileBackend`.
///
/// Uses the registered value from [`init_workspace`] if set; otherwise falls
/// back to the same env-var / home-dir logic as the config subsystem.
fn workspace_dir_for_file_backend() -> PathBuf {
    if let Some(dir) = WORKSPACE_DIR.get() {
        return dir.clone();
    }

    // Fallback: replicate config's default derivation.
    //   OPENHUMAN_WORKSPACE → use directly.
    //   Else home_dir/.openhuman-staging/workspace (staging) or
    //        home_dir/.openhuman/workspace (default).
    if let Ok(custom) = std::env::var("OPENHUMAN_WORKSPACE") {
        return PathBuf::from(custom);
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let openhuman_dir = match std::env::var("OPENHUMAN_APP_ENV").as_deref() {
        Ok("staging") => home.join(".openhuman-staging"),
        _ => home.join(".openhuman"),
    };
    openhuman_dir.join("workspace")
}

// ── Outcome type ─────────────────────────────────────────────────────────────

/// Outcome of a file-to-keychain migration attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// A keychain entry already existed — no action taken.
    AlreadyMigrated,
    /// Source file was read, stored in keychain, verified, and then deleted.
    MigratedAndDeleted,
    /// Source file did not exist; nothing to migrate.
    NoSourceFile,
}

// ── Core operations ───────────────────────────────────────────────────────────

/// Retrieve a secret from the active backend.
///
/// Returns `Ok(None)` when no entry exists for this user + key combination.
/// Never logs the secret value.
pub fn get(user_id: &str, key: &str) -> Result<Option<String>, KeyringError> {
    log::debug!("[keyring] get user_id={user_id} key={key}");
    let namespaced = namespaced_key(user_id, key);
    let result = backend().get(&namespaced);
    match &result {
        Ok(Some(_)) => log::debug!("[keyring] get hit user_id={user_id} key={key}"),
        Ok(None) => log::debug!("[keyring] get miss user_id={user_id} key={key}"),
        Err(e) => log::warn!("[keyring] get error user_id={user_id} key={key}: {e}"),
    }
    result
}

/// Store a secret in the active backend.
///
/// Overwrites any existing entry for this user + key. Never logs the value.
pub fn set(user_id: &str, key: &str, value: &str) -> Result<(), KeyringError> {
    log::debug!("[keyring] set user_id={user_id} key={key}");
    let namespaced = namespaced_key(user_id, key);
    let result = backend().set(&namespaced, value);
    match &result {
        Ok(()) => log::debug!("[keyring] set ok user_id={user_id} key={key}"),
        Err(e) => log::warn!("[keyring] set error user_id={user_id} key={key}: {e}"),
    }
    result
}

/// Delete a secret from the active backend.
///
/// Returns `Ok(())` even if no entry existed (idempotent).
pub fn delete(user_id: &str, key: &str) -> Result<(), KeyringError> {
    log::debug!("[keyring] delete user_id={user_id} key={key}");
    let namespaced = namespaced_key(user_id, key);
    let result = backend().delete(&namespaced);
    match &result {
        Ok(()) => log::debug!("[keyring] delete ok user_id={user_id} key={key}"),
        Err(e) => log::warn!("[keyring] delete error user_id={user_id} key={key}: {e}"),
    }
    result
}

/// Probe whether the active backend is usable on this machine.
///
/// For the `file` and `mock` backends this always returns `true`.  For the
/// `os` backend on Linux headless systems (no Secret Service daemon) this
/// returns `false`; callers should fall back to file-based storage.
pub fn is_available() -> bool {
    const PROBE_USER: &str = "__probe__";
    const PROBE_KEY: &str = "__openhuman_keyring_probe__";
    const PROBE_VALUE: &str = "__probe_value__";

    log::debug!("[keyring] is_available probe starting backend={}", backend().name());

    // File and mock backends are always available.
    let b = backend();
    if b.name() == "file" || b.name() == "mock" {
        log::debug!("[keyring] is_available=true (non-os backend)");
        return true;
    }

    let result = (|| -> Result<bool, KeyringError> {
        set(PROBE_USER, PROBE_KEY, PROBE_VALUE)?;
        let readback = get(PROBE_USER, PROBE_KEY)?;
        delete(PROBE_USER, PROBE_KEY)?;
        Ok(readback.as_deref() == Some(PROBE_VALUE))
    })();

    match result {
        Ok(ok) => {
            log::debug!("[keyring] is_available={ok}");
            ok
        }
        Err(e) => {
            log::debug!("[keyring] is_available=false (probe failed): {e}");
            false
        }
    }
}

/// Retrieve or generate-and-store a random hex secret of `len_bytes` bytes.
///
/// If an entry already exists it is returned unchanged (idempotent).
/// If no entry exists a fresh random value is generated, stored, and returned.
///
/// The returned value is a lowercase hex string of length `len_bytes * 2`.
pub fn get_or_create_random(
    user_id: &str,
    key: &str,
    len_bytes: usize,
) -> Result<String, KeyringError> {
    log::debug!("[keyring] get_or_create_random user_id={user_id} key={key} len_bytes={len_bytes}");

    if let Some(existing) = get(user_id, key)? {
        log::debug!("[keyring] get_or_create_random returning existing value user_id={user_id} key={key}");
        return Ok(existing);
    }

    // Generate random bytes using the OS CSPRNG.
    let mut bytes = vec![0u8; len_bytes];
    OsRng.fill_bytes(&mut bytes);
    let hex_value = hex_encode(&bytes);

    log::debug!("[keyring] get_or_create_random creating new entry user_id={user_id} key={key}");
    set(user_id, key, &hex_value)?;

    // Verify write succeeded.
    let readback = get(user_id, key)?;
    if readback.as_deref() != Some(&hex_value) {
        log::warn!("[keyring] get_or_create_random write verification failed user_id={user_id} key={key}");
        return Err(KeyringError::VerifyFailed { key: key.to_string() });
    }

    log::debug!("[keyring] get_or_create_random created and verified user_id={user_id} key={key}");
    Ok(hex_value)
}

/// Migrate a secret from a file into the active backend.
///
/// Semantics:
/// - If an entry already exists → [`MigrationOutcome::AlreadyMigrated`].
/// - If no entry but `path` exists → read, store, verify, delete file →
///   [`MigrationOutcome::MigratedAndDeleted`].
/// - If neither exists → [`MigrationOutcome::NoSourceFile`].
///
/// On any failure after the file has been read but before it can be deleted,
/// the file is **not** deleted and `Err` is returned so the caller can retry.
pub fn migrate_from_file(
    user_id: &str,
    key: &str,
    path: &Path,
) -> Result<MigrationOutcome, KeyringError> {
    log::debug!(
        "[keyring] migrate_from_file user_id={user_id} key={key} path={}",
        path.display()
    );

    // Step 1: check if already migrated.
    if get(user_id, key)?.is_some() {
        log::debug!(
            "[keyring] migrate_from_file already migrated user_id={user_id} key={key}"
        );
        return Ok(MigrationOutcome::AlreadyMigrated);
    }

    // Step 2: check if source file exists.
    if !path.exists() {
        log::debug!(
            "[keyring] migrate_from_file no source file user_id={user_id} key={key} path={}",
            path.display()
        );
        return Ok(MigrationOutcome::NoSourceFile);
    }

    // Step 3: read the file.
    log::debug!(
        "[keyring] migrate_from_file reading source file path={}",
        path.display()
    );
    let file_content = std::fs::read_to_string(path).map_err(|e| {
        KeyringError::MigrationReadFailed {
            path: path.display().to_string(),
            source: e,
        }
    })?;
    let value = file_content.trim().to_string();

    // Step 4: write to backend.
    log::debug!(
        "[keyring] migrate_from_file writing to backend user_id={user_id} key={key}"
    );
    set(user_id, key, &value)?;

    // Step 5: verify read-back matches.
    let readback = get(user_id, key)?;
    if readback.as_deref() != Some(value.as_str()) {
        log::warn!(
            "[keyring] migrate_from_file verification failed user_id={user_id} key={key}; NOT deleting source file"
        );
        return Err(KeyringError::VerifyFailed { key: key.to_string() });
    }

    // Step 6: delete the source file (only after verified write).
    log::debug!(
        "[keyring] migrate_from_file deleting source file path={}",
        path.display()
    );
    std::fs::remove_file(path).map_err(|e| KeyringError::MigrationDeleteFailed {
        path: path.display().to_string(),
        source: e,
    })?;

    log::info!(
        "[keyring] migrate_from_file completed user_id={user_id} key={key} path={}",
        path.display()
    );
    Ok(MigrationOutcome::MigratedAndDeleted)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Produce the namespaced key used inside the backend store.
///
/// Format: `"{user_id}:{key}"`.  This ensures user A's keys are never
/// reachable as user B's keys.
fn namespaced_key(user_id: &str, key: &str) -> String {
    format!("{user_id}:{key}")
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Force-reset the backend to a fresh `MockBackend`.
///
/// Only available in `#[cfg(test)]`.  Call this at the top of each test that
/// needs keyring isolation.  Because `OnceLock` cannot be reset, this function
/// panics if the backend was already initialized — tests that use it must
/// ensure they run in separate processes (i.e. each test binary gets a fresh
/// `OnceLock`) or call [`force_backend_for_test`] before any keyring call.
#[cfg(test)]
pub(crate) fn force_backend_for_test(b: Box<dyn KeyringBackend>) {
    // BACKEND is a OnceLock; we can't reset it after initialization.
    // Tests that need isolation should use the env-var path or a tempdir
    // FileBackend directly via backend::FileBackend::new().
    let _ = BACKEND.set(b);
}
