//! Shared plumbing for the keyring backends that sit over
//! `tinystoragedrivers`' `SecretStore` port: error mapping, UTF-8 decoding
//! and the desktop corruption policy.

use std::path::Path;

use tinystoragedrivers::secrets::{EncryptedFileSecrets, SecretStore as _};
use tinystoragedrivers::{ErrorKind, StorageError};
use zeroize::Zeroizing;

use crate::security::keyring::error::KeyringError;
use crate::security::keyring::file_store;

/// A driver error as a [`KeyringError::Backend`], for the file store.
pub(super) fn backend_error(error: StorageError) -> KeyringError {
    KeyringError::Backend(format!("secret store: {error}"))
}

/// A driver error as the [`KeyringError::Os`] the OS-keychain callers (and
/// the credential-ref error classifier) have always seen. A locked or denied
/// store keeps its `NoStorageAccess` identity; the driver's error rides along
/// as the source (it never carries a secret value).
pub(super) fn os_error(key: &str, error: StorageError) -> KeyringError {
    let source = if error.kind() == ErrorKind::Unavailable {
        keyring::Error::NoStorageAccess(Box::new(error))
    } else {
        keyring::Error::PlatformFailure(Box::new(error))
    };
    KeyringError::Os {
        key: key.to_string(),
        source,
    }
}

/// Whether the keychain refused access rather than failed: reads and deletes
/// treat that as "nothing there", as they always have.
pub(super) fn is_no_access(error: &StorageError) -> bool {
    error.kind() == ErrorKind::Unavailable
}

/// Whether the driver reports the secrets file itself as unreadable
/// (undecryptable under this key, or not the expected JSON).
pub(super) fn is_corruption(error: &StorageError) -> bool {
    matches!(error.kind(), ErrorKind::Crypto | ErrorKind::Serialization)
}

/// Decode a stored value as UTF-8 text.
pub(super) fn utf8(
    key: &str,
    value: Option<Zeroizing<Vec<u8>>>,
) -> Result<Option<String>, KeyringError> {
    value
        .map(|bytes| {
            String::from_utf8(bytes.to_vec()).map_err(|source| KeyringError::InvalidUtf8 {
                key: key.to_string(),
                source,
            })
        })
        .transpose()
}

/// Desktop policy for a `secrets.enc` the driver could not read: move it
/// aside and let the caller continue on an empty store.
///
/// The driver fails closed (error, file untouched), which suits a server with
/// an operator but would wedge a desktop: every write fails, so the user can
/// never sign in again. Today's behaviour is kept instead: quarantine the
/// bytes (`secrets.enc.corrupt.<ts>`, never deleted) and log loudly.
///
/// Under the same cross-process lock writers take, the file is probed again
/// first: a concurrent writer may already have replaced it, and quarantining
/// that replacement would destroy good secrets.
///
/// # Errors
///
/// When the file cannot be moved aside. Continuing would overwrite secrets
/// that could not be read, so the operation fails closed instead.
pub(super) fn recover_corrupt_file(
    path: &Path,
    key: &[u8; 32],
    cause: &StorageError,
) -> Result<(), KeyringError> {
    let _guard = file_store::lock_for_write(path)?;
    let store = EncryptedFileSecrets::at_path(path, Zeroizing::new(*key));
    match crate::storage::block_on(async move { store.list("").await }) {
        Ok(_) => {
            log::info!("[keyring:encrypted_file] secrets file readable again; not quarantining");
            return Ok(());
        }
        // Still corrupt: fall through to quarantine.
        Err(error) if is_corruption(&error) => {}
        // Anything else (a transient read or permission error) says nothing
        // about the file's contents: never move a possibly healthy file aside.
        Err(error) => return Err(backend_error(error)),
    }
    log::error!(
        "[keyring:encrypted_file] secrets file unreadable ({}): master key may have changed \
         or the file is corrupt",
        cause.message()
    );
    if file_store::quarantine_corrupt(path, "enc").is_none() {
        return Err(KeyringError::Backend(
            "secrets file is unreadable and could not be moved aside".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
pub(crate) mod tests;
