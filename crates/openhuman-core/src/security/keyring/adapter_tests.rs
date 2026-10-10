//! The keyring backends over the `SecretStore` port: the OS backend against a
//! persistent fake credential store (never the real keychain), the encrypted
//! file backend's desktop corruption policy, and the legacy-file import.

use super::*;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use tinystoragedrivers::secrets::{crypto, KeyringSecrets};

use crate::security::keyring::backend::OsBackend;
use crate::security::keyring::encrypted_file_backend::EncryptedFileBackend;
use crate::security::keyring::KeyringBackend;

type Shared = Arc<Mutex<HashMap<(String, String), Vec<u8>>>>;

/// A credential store shared by every credential the builder makes, so a set
/// through one `Entry` is visible through the next, as on a real keychain.
#[derive(Debug, Default)]
struct FakeBuilder {
    store: Shared,
    locked: bool,
}

#[derive(Debug)]
struct FakeCredential {
    store: Shared,
    id: (String, String),
    locked: bool,
}

impl CredentialBuilderApi for FakeBuilder {
    fn build(
        &self,
        _t: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(FakeCredential {
            store: Arc::clone(&self.store),
            id: (service.to_string(), user.to_string()),
            locked: self.locked,
        }))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn denied() -> keyring::Error {
    keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("locked")))
}

impl CredentialApi for FakeCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        if self.locked {
            return Err(denied());
        }
        self.store
            .lock()
            .unwrap()
            .insert(self.id.clone(), secret.to_vec());
        Ok(())
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        if self.locked {
            return Err(denied());
        }
        self.store
            .lock()
            .unwrap()
            .get(&self.id)
            .cloned()
            .ok_or(keyring::Error::NoEntry)
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        if self.locked {
            return Err(denied());
        }
        self.store
            .lock()
            .unwrap()
            .remove(&self.id)
            .map(|_| ())
            .ok_or(keyring::Error::NoEntry)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A persistent fake OS keychain under the production service name.
pub(crate) fn fake_os_backend() -> OsBackend {
    OsBackend::with_store(KeyringSecrets::with_credential_builder(
        "openhuman",
        Box::new(FakeBuilder::default()),
    ))
}

fn locked_os_backend() -> OsBackend {
    OsBackend::with_store(KeyringSecrets::with_credential_builder(
        "openhuman",
        Box::new(FakeBuilder {
            locked: true,
            ..Default::default()
        }),
    ))
}

// ── OsBackend ────────────────────────────────────────────────────────────────

#[test]
fn os_backend_round_trips_through_the_port() {
    let os = fake_os_backend();
    assert_eq!(os.get("u:k").unwrap(), None);
    os.set("u:k", "v1").unwrap();
    os.set("u:k", "v2").unwrap();
    assert_eq!(os.get("u:k").unwrap().as_deref(), Some("v2"));
    os.delete("u:k").unwrap();
    os.delete("u:k").unwrap();
    assert_eq!(os.get("u:k").unwrap(), None);
    assert_eq!(os.name(), "os");
}

#[test]
fn os_backend_keeps_the_credential_layout() {
    // Service `openhuman`, user `{user_id}:{key}`, the password as the value:
    // what the previous direct `keyring::Entry` implementation wrote.
    let builder = FakeBuilder::default();
    let store = Arc::clone(&builder.store);
    let os = OsBackend::with_store(KeyringSecrets::with_credential_builder(
        "openhuman",
        Box::new(builder),
    ));
    os.set("alice:token", "s3cret").unwrap();
    let held = store.lock().unwrap();
    assert_eq!(
        held[&("openhuman".to_string(), "alice:token".to_string())],
        b"s3cret"
    );
}

#[test]
fn a_locked_keychain_reads_empty_and_deletes_quietly_but_fails_a_write() {
    let os = locked_os_backend();
    assert_eq!(os.get("u:k").unwrap(), None);
    os.delete("u:k").unwrap();
    let error = os.set("u:k", "v").unwrap_err();
    assert!(
        matches!(
            error,
            KeyringError::Os {
                source: keyring::Error::NoStorageAccess(_),
                ..
            }
        ),
        "{error:?}"
    );
}

// ── EncryptedFileBackend ─────────────────────────────────────────────────────

const KEY: [u8; 32] = [7; 32];

#[test]
fn file_backend_round_trips_and_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let b = EncryptedFileBackend::new(dir.path());
    assert_eq!(b.get_with_key(&KEY, "u:a").unwrap(), None);
    b.set_with_key(&KEY, "u:a", "1").unwrap();
    b.set_with_key(&KEY, "u:b", "2").unwrap();
    assert_eq!(b.get_with_key(&KEY, "u:a").unwrap().as_deref(), Some("1"));
    let blob = std::fs::read(dir.path().join("secrets.enc")).unwrap();
    let map: HashMap<String, String> =
        serde_json::from_slice(&crypto::decrypt(&KEY, &blob).unwrap()).unwrap();
    assert_eq!(map.len(), 2, "one blob over a json object of strings");
}

fn quarantined(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("secrets.enc.corrupt."))
        .collect()
}

#[test]
fn a_file_under_another_key_is_quarantined_and_the_store_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let b = EncryptedFileBackend::new(dir.path());
    b.set_with_key(&[9; 32], "u:old", "old-value").unwrap();
    let original = std::fs::read(dir.path().join("secrets.enc")).unwrap();

    // Desktop policy, unlike the driver's fail-closed: log, move aside, go on.
    assert_eq!(b.get_with_key(&KEY, "u:old").unwrap(), None);
    let moved = quarantined(dir.path());
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(
        std::fs::read(dir.path().join(&moved[0])).unwrap(),
        original,
        "the bytes are preserved, not deleted"
    );
    b.set_with_key(&KEY, "u:new", "fresh").unwrap();
    assert_eq!(
        b.get_with_key(&KEY, "u:new").unwrap().as_deref(),
        Some("fresh")
    );
}

#[test]
fn a_garbage_file_does_not_wedge_writes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("secrets.enc"),
        b"definitely not a blob at all, no",
    )
    .unwrap();
    let b = EncryptedFileBackend::new(dir.path());
    b.set_with_key(&KEY, "u:a", "1").unwrap();
    assert_eq!(b.get_with_key(&KEY, "u:a").unwrap().as_deref(), Some("1"));
    assert_eq!(quarantined(dir.path()).len(), 1);
}

#[test]
fn an_undecodable_payload_is_quarantined_too() {
    // Decrypts, but is not a json object of strings.
    let dir = tempfile::tempdir().unwrap();
    let blob = crypto::encrypt(&KEY, b"[1,2,3]").unwrap();
    std::fs::write(dir.path().join("secrets.enc"), blob).unwrap();
    let b = EncryptedFileBackend::new(dir.path());
    assert_eq!(b.get_with_key(&KEY, "u:a").unwrap(), None);
    assert_eq!(quarantined(dir.path()).len(), 1);
}

#[test]
fn a_legacy_dev_keychain_is_imported_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("dev-keychain.json"),
        br#"{"u:legacy":"from-json"}"#,
    )
    .unwrap();
    let b = EncryptedFileBackend::new(dir.path());
    assert_eq!(
        b.get_with_key(&KEY, "u:legacy").unwrap().as_deref(),
        Some("from-json")
    );
    assert!(!dir.path().join("dev-keychain.json").exists());
    assert!(dir.path().join("dev-keychain.json.migrated").exists());
    assert!(dir.path().join("secrets.enc").exists());
}

#[test]
fn recovery_never_quarantines_a_file_that_is_not_confirmed_corrupt() {
    // The re-probe fails with an I/O error (the path is a directory), not a
    // crypto/serialization one: nothing says the contents are bad, so nothing
    // is moved and the error is surfaced.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.enc");
    std::fs::create_dir(&path).unwrap();
    let cause = tinystoragedrivers::StorageError::crypto("earlier failure");
    let error = recover_corrupt_file(&path, &KEY, &cause).unwrap_err();
    assert!(matches!(error, KeyringError::Backend(_)), "{error:?}");
    assert!(path.is_dir(), "left in place");
}

#[test]
fn recovery_leaves_a_file_that_became_readable() {
    let dir = tempfile::tempdir().unwrap();
    let b = EncryptedFileBackend::new(dir.path());
    b.set_with_key(&KEY, "u:a", "1").unwrap();
    let cause = tinystoragedrivers::StorageError::crypto("stale failure");
    recover_corrupt_file(&dir.path().join("secrets.enc"), &KEY, &cause).unwrap();
    assert_eq!(b.get_with_key(&KEY, "u:a").unwrap().as_deref(), Some("1"));
}
