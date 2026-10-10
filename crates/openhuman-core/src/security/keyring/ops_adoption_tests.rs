//! Migration from the process/OS keyring into storage-backed secrets, with a
//! fake keychain standing in for the OS.

use super::*;
use std::sync::Arc;

use tinystoragedrivers::secrets::{DerivedKeys, SecretStore as _};
use zeroize::Zeroizing;

use crate::security::keyring::adapter::tests::fake_os_backend;
use crate::security::keyring::KeyringBackend;
use crate::storage::{MemoryStorage, Scope, StorageBackend};

fn storage_secrets_in(
    storage: &MemoryStorage,
    scope: &str,
) -> tinystoragedrivers::secrets::DocumentSecrets {
    crate::storage::secrets::over(
        &storage.for_scope(&Scope::new(scope).unwrap()).unwrap(),
        Arc::new(DerivedKeys::new(Zeroizing::new([5; 32]))),
    )
}

#[test]
fn an_os_keyring_secret_is_adopted_into_storage_on_first_read() {
    let os = fake_os_backend();
    os.set("alice:token", "from-the-os-keychain").unwrap();
    let storage = MemoryStorage::new();
    let secrets = storage_secrets_in(&storage, "local");

    let read = |secrets| {
        get_adopting(secrets, "alice:token", "token", || {
            os.get("alice:token").ok().flatten()
        })
        .unwrap()
    };
    assert_eq!(
        read(secrets.clone()).as_deref(),
        Some("from-the-os-keychain")
    );

    // It now lives in storage, so it survives the OS copy going away.
    os.delete("alice:token").unwrap();
    assert_eq!(
        read(secrets.clone()).as_deref(),
        Some("from-the-os-keychain")
    );
    let held = crate::storage::block_on(async move { secrets.get("alice:token").await }).unwrap();
    assert_eq!(held.unwrap().as_slice(), b"from-the-os-keychain");
}

#[test]
fn a_secret_absent_everywhere_stays_absent_and_storage_wins_over_the_os() {
    let os = fake_os_backend();
    let storage = MemoryStorage::new();
    let secrets = storage_secrets_in(&storage, "local");
    assert_eq!(
        get_adopting(secrets.clone(), "bob:k", "k", || os
            .get("bob:k")
            .ok()
            .flatten())
        .unwrap(),
        None
    );

    let s2 = secrets.clone();
    crate::storage::block_on(async move { s2.set("bob:k", b"stored").await }).unwrap();
    os.set("bob:k", "stale-os").unwrap();
    assert_eq!(
        get_adopting(secrets, "bob:k", "k", || os.get("bob:k").ok().flatten())
            .unwrap()
            .as_deref(),
        Some("stored")
    );
}

#[test]
fn adopted_secrets_do_not_cross_scopes() {
    let os = fake_os_backend();
    os.set("u:k", "v").unwrap();
    let storage = MemoryStorage::new();
    get_adopting(storage_secrets_in(&storage, "alice"), "u:k", "k", || {
        os.get("u:k").ok().flatten()
    })
    .unwrap();
    let other = storage_secrets_in(&storage, "bob");
    assert_eq!(
        crate::storage::block_on(async move { other.get("u:k").await }).unwrap(),
        None
    );
}
