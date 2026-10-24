//! The credential fallback files a profile keeps beside `config.toml`.
//!
//! `auth-profiles.json` and `http-credentials.json` are the on-disk fallbacks
//! the credential stores use when no storage backend holds them. Under layout
//! v2 they live as encrypted secrets (`tsd.secrets`, `enc2:` ChaCha20-Poly1305,
//! a per-scope key derived from the process master key) in the profile's
//! `local` scope. Step 2 of the migration imports them there, verifies the
//! read-back, and lists them in the plan as deleted entries so the archive
//! holds them and the final step removes them. A file that is not imported
//! (no usable master key, not valid JSON) is never classified, so it is left
//! in place and the stores keep adopting it lazily.
//!
//! The process-level files stay where they are and are outside every profile's
//! plan: `keyring/master.key` and the shared `secrets.enc` keyring (one file
//! for all profiles, entries namespaced by user id, adopted per key on first
//! read by `secret_storage::get`). The master key is never written into
//! `storage.db`.

use std::path::Path;

#[cfg(feature = "legacy-import")]
use tinystoragedrivers::secrets::SecretStore as _;

use super::state::LayoutError;
#[cfg(feature = "legacy-import")]
use super::Target;
use crate::security::credentials::http_creds::{self, STORAGE_SECRET_NAME, STORE_FILENAME};
use crate::security::credentials::profiles::{self, PROFILES_FILENAME, PROFILES_SECRET_NAME};
#[cfg(feature = "legacy-import")]
use crate::storage;

/// The path prefix a profile-level plan entry carries (`@profile/<file>`),
/// to tell it from an entry below `workspace/`.
pub const PROFILE_PREFIX: &str = "@profile/";

/// (file name beside `config.toml`, secret name on the storage backend).
pub const FILES: [(&str, &str); 2] = [
    (PROFILES_FILENAME, PROFILES_SECRET_NAME),
    (STORE_FILENAME, STORAGE_SECRET_NAME),
];

/// A credential file the import would take: a regular file beside
/// `config.toml` holding the document its store persists. `None` otherwise
/// (missing, a symlink, or not that document: left in place).
fn eligible(
    profile_dir: &Path,
    file: &str,
) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, LayoutError> {
    let path = profile_dir.join(file);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        tracing::warn!(target: "openhuman::storage", file, "[layout] credential entry is not a regular file; left in place");
        return Ok(None);
    }
    // A read that fails for any reason but disappearance aborts, so the
    // migration stays resumable instead of publishing v2 without the file.
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => zeroize::Zeroizing::new(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            tracing::warn!(target: "openhuman::storage", file, error = %e, "[layout] credential file unreadable");
            return Err(e.into());
        }
    };
    let valid = if file == PROFILES_FILENAME {
        profiles::is_persisted_document(&bytes)
    } else {
        http_creds::is_persisted_document(&bytes)
    };
    if !valid {
        tracing::warn!(target: "openhuman::storage", file, "[layout] credential file is not a valid credential document; left in place");
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// The credential files a migration of `profile_dir` would import: the
/// eligible ones, and none when no master key is resolvable. Read-only; the
/// dry run uses it so its plan matches the run.
pub fn candidates(profile_dir: &Path) -> Result<Vec<String>, LayoutError> {
    if !crate::security::secret_storage::encrypted_file_backend::storage_master_key_resolvable() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for (file, _) in FILES {
        if eligible(profile_dir, file)?.is_some() {
            found.push(file.to_owned());
        }
    }
    Ok(found)
}

/// Imports the credential files of `target` into the v1 database's `local`
/// scope and returns the file names that are now safe to archive and delete.
///
/// A record already on the backend is authoritative (a v1 core with the
/// backend installed adopts and then updates it there, so the file may be
/// stale); it is only checked to be readable with the current key.
///
/// # Errors
///
/// A write that does not read back identically, or a stored record that does
/// not decrypt with the current key. Nothing is deleted then.
#[cfg(feature = "legacy-import")]
pub async fn import(target: &Target) -> Result<Vec<String>, LayoutError> {
    let mut files = Vec::new();
    for (file, secret) in FILES {
        if let Some(bytes) = eligible(&target.profile_dir, file)? {
            files.push((file, secret, bytes));
        }
    }
    if files.is_empty() {
        return Ok(Vec::new());
    }
    if !crate::security::secret_storage::encrypted_file_backend::storage_master_key_resolvable() {
        tracing::warn!(
            target: "openhuman::storage",
            "[layout] no master key to encrypt the credential files with; left in place"
        );
        return Ok(Vec::new());
    }
    let keys = storage::secrets::keys()
        .map_err(|e| LayoutError::Other(anyhow::anyhow!("master key for credentials: {e}")))?;
    let backend = super::legacy::open_v1(target).await?;
    let scope = storage::tenant_scope(None);
    let scoped = backend
        .for_scope(&scope)
        .map_err(|e| LayoutError::Other(anyhow::anyhow!("open the local scope: {e}")))?;
    let secrets = storage::secrets::over(&scoped, scope, keys);
    let mut imported = Vec::new();
    for (file, secret, bytes) in files {
        let stored = secrets.get(secret).await.map_err(|e| {
            LayoutError::Verify(format!("stored credential record for {file}: {e}"))
        })?;
        if stored.is_none() {
            secrets
                .set(secret, &bytes)
                .await
                .map_err(|e| LayoutError::Other(anyhow::anyhow!("store {file}: {e}")))?;
            let back = secrets
                .get(secret)
                .await
                .map_err(|e| LayoutError::Verify(format!("read back {file}: {e}")))?;
            if back.as_deref().map(|b| b.as_slice()) != Some(bytes.as_slice()) {
                return Err(LayoutError::Verify(format!(
                    "{file} did not read back identically; refusing to delete it"
                )));
            }
        }
        tracing::info!(target: "openhuman::storage", file, "[layout] credential file imported");
        imported.push(file.to_owned());
    }
    drop(backend);
    Ok(imported)
}

#[cfg(all(test, feature = "legacy-import", feature = "storage-sqlite"))]
#[path = "credentials_tests.rs"]
mod tests;
