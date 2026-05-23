//! Backend implementations for the keyring module.
//!
//! Two concrete backends are provided:
//!
//! - [`OsBackend`]: Wraps the `keyring` crate to use the native OS credential
//!   store (macOS Keychain, Windows Credential Manager, Linux Secret Service).
//!   This is the production backend.
//!
//! - [`FileBackend`]: Stores secrets in a plain JSON file at
//!   `{workspace}/dev-keychain.json`.  **This file is NOT encrypted** — it is a
//!   development artifact only and must never be used in production.  It exists
//!   solely to avoid the "different binary signature → macOS Keychain permission
//!   prompt on every `cargo run`" problem that plagues dev workflows.
//!
//! Backend selection happens once at first use (see [`super::selected_backend`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::openhuman::keyring::error::KeyringError;

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstraction over a secret-storage backend.
///
/// All implementations must be `Send + Sync` so they can live inside a
/// `OnceLock<Box<dyn KeyringBackend>>`.
pub trait KeyringBackend: Send + Sync {
    /// Retrieve a secret.  Returns `Ok(None)` when no entry exists.
    fn get(&self, namespaced_key: &str) -> Result<Option<String>, KeyringError>;
    /// Store (or overwrite) a secret.
    fn set(&self, namespaced_key: &str, value: &str) -> Result<(), KeyringError>;
    /// Delete a secret.  Must be idempotent (no error if the entry is absent).
    fn delete(&self, namespaced_key: &str) -> Result<(), KeyringError>;
    /// Human-readable name used in log lines.
    fn name(&self) -> &'static str;
}

// ── OsBackend ─────────────────────────────────────────────────────────────────

/// Production backend: native OS credential store via the `keyring` crate.
pub struct OsBackend;

const SERVICE_NAME: &str = "openhuman";

impl KeyringBackend for OsBackend {
    fn get(&self, namespaced_key: &str) -> Result<Option<String>, KeyringError> {
        let entry = keyring::Entry::new(SERVICE_NAME, namespaced_key)
            .map_err(|e| KeyringError::Os { key: namespaced_key.to_string(), source: e })?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(keyring::Error::NoStorageAccess(_)) => Ok(None),
            Err(e) => Err(KeyringError::Os { key: namespaced_key.to_string(), source: e }),
        }
    }

    fn set(&self, namespaced_key: &str, value: &str) -> Result<(), KeyringError> {
        let entry = keyring::Entry::new(SERVICE_NAME, namespaced_key)
            .map_err(|e| KeyringError::Os { key: namespaced_key.to_string(), source: e })?;
        entry
            .set_password(value)
            .map_err(|e| KeyringError::Os { key: namespaced_key.to_string(), source: e })
    }

    fn delete(&self, namespaced_key: &str) -> Result<(), KeyringError> {
        let entry = keyring::Entry::new(SERVICE_NAME, namespaced_key)
            .map_err(|e| KeyringError::Os { key: namespaced_key.to_string(), source: e })?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(keyring::Error::NoStorageAccess(_)) => Ok(()),
            Err(e) => Err(KeyringError::Os { key: namespaced_key.to_string(), source: e }),
        }
    }

    fn name(&self) -> &'static str {
        "os"
    }
}

// ── FileBackend ───────────────────────────────────────────────────────────────

/// Dev-only backend: plain JSON file at `{workspace}/dev-keychain.json`.
///
/// # WARNING — NOT FOR PRODUCTION
///
/// Secrets stored here are **not encrypted**.  This backend exists only to
/// eliminate macOS Keychain permission prompts during development (where the
/// binary signature changes on every `cargo build`).  It is selected
/// automatically in debug builds and when `OPENHUMAN_APP_ENV=dev|staging`.
/// Never use it in a production deployment.
pub struct FileBackend {
    path: PathBuf,
}

impl FileBackend {
    /// Create a `FileBackend` that reads/writes `{workspace_dir}/dev-keychain.json`.
    pub fn new(workspace_dir: &Path) -> Self {
        Self { path: workspace_dir.join("dev-keychain.json") }
    }

    /// Path to the backing file (exposed for logging).
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_map(&self) -> Result<HashMap<String, String>, KeyringError> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let bytes = std::fs::read(&self.path).map_err(|e| {
            KeyringError::MigrationReadFailed {
                path: self.path.display().to_string(),
                source: e,
            }
        })?;
        if bytes.is_empty() {
            return Ok(HashMap::new());
        }
        serde_json::from_slice::<HashMap<String, String>>(&bytes).map_err(|e| {
            // Treat a corrupt file as empty so we degrade gracefully.
            log::warn!(
                "[keyring] dev-keychain.json at {} is corrupt ({e}); treating as empty",
                self.path.display()
            );
            // Return empty map by converting to a no-source variant.
            drop(e);
            KeyringError::VerifyFailed { key: "<parse>".to_string() }
        }).or_else(|_| Ok(HashMap::new()))
    }

    fn write_map(&self, map: &HashMap<String, String>) -> Result<(), KeyringError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| KeyringError::MigrationReadFailed {
                path: parent.display().to_string(),
                source: e,
            })?;
        }

        // Serialize the map to pretty JSON.  HashMap<String,String> is always
        // serializable by serde_json, so the unwrap_or_default fallback is a
        // safety net that should never be reached in practice.
        let json = serde_json::to_vec_pretty(map).unwrap_or_default();

        // Atomic write: temp file + rename.
        let tmp_path = self.path.with_extension("tmp");
        std::fs::write(&tmp_path, &json).map_err(|e| KeyringError::MigrationDeleteFailed {
            path: tmp_path.display().to_string(),
            source: e,
        })?;

        // Set mode 0600 on Unix before moving into place.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            if let Err(e) = std::fs::set_permissions(&tmp_path, perms) {
                log::warn!(
                    "[keyring] could not set 0600 on dev-keychain.json tmp file: {e}"
                );
            }
        }

        std::fs::rename(&tmp_path, &self.path).map_err(|e| KeyringError::MigrationDeleteFailed {
            path: self.path.display().to_string(),
            source: e,
        })?;

        Ok(())
    }
}

impl KeyringBackend for FileBackend {
    fn get(&self, namespaced_key: &str) -> Result<Option<String>, KeyringError> {
        let map = self.read_map()?;
        Ok(map.get(namespaced_key).cloned())
    }

    fn set(&self, namespaced_key: &str, value: &str) -> Result<(), KeyringError> {
        let mut map = self.read_map()?;
        map.insert(namespaced_key.to_string(), value.to_string());
        self.write_map(&map)
    }

    fn delete(&self, namespaced_key: &str) -> Result<(), KeyringError> {
        let mut map = self.read_map()?;
        if map.remove(namespaced_key).is_some() {
            self.write_map(&map)?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "file"
    }
}

// ── MockBackend (test only) ───────────────────────────────────────────────────

/// In-memory backend used in tests and when `OPENHUMAN_KEYRING_BACKEND=mock`.
#[cfg(test)]
pub struct MockBackend {
    store: std::sync::Mutex<HashMap<String, String>>,
}

#[cfg(test)]
impl MockBackend {
    pub fn new() -> Self {
        Self { store: std::sync::Mutex::new(HashMap::new()) }
    }
}

#[cfg(test)]
impl KeyringBackend for MockBackend {
    fn get(&self, namespaced_key: &str) -> Result<Option<String>, KeyringError> {
        Ok(self.store.lock().unwrap().get(namespaced_key).cloned())
    }

    fn set(&self, namespaced_key: &str, value: &str) -> Result<(), KeyringError> {
        self.store.lock().unwrap().insert(namespaced_key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, namespaced_key: &str) -> Result<(), KeyringError> {
        self.store.lock().unwrap().remove(namespaced_key);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}
