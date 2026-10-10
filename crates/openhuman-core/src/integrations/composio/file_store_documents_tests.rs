use super::*;
use crate::storage::{MemoryStorage, Scope, StorageBackend};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn docs_in(storage: &MemoryStorage, scope: &str) -> Docs {
    Docs::over(&storage.for_scope(&Scope::new(scope).unwrap()).unwrap())
}

fn path() -> PathBuf {
    PathBuf::from("/ws/integrations/composio_identities.json")
}

#[test]
fn a_missing_file_loads_the_default() {
    let loaded: BTreeMap<String, u32> = docs_in(&MemoryStorage::new(), "local")
        .load(&path())
        .unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn save_then_load_round_trips_by_file_name() {
    let docs = docs_in(&MemoryStorage::new(), "local");
    let mut value = BTreeMap::new();
    value.insert("gmail".to_string(), 3u32);
    docs.save(&path(), &value).unwrap();
    // The workspace part of the path is irrelevant; the file name is the key.
    let loaded: BTreeMap<String, u32> = docs
        .load(&PathBuf::from("/elsewhere/composio_identities.json"))
        .unwrap();
    assert_eq!(loaded, value);
    let other: BTreeMap<String, u32> = docs
        .load(&PathBuf::from("/ws/integrations/composio_user_scopes.json"))
        .unwrap();
    assert!(other.is_empty());
}

#[test]
fn scopes_do_not_see_each_others_state() {
    let storage = MemoryStorage::new();
    let mut value = BTreeMap::new();
    value.insert("gmail".to_string(), 1u32);
    docs_in(&storage, "alice").save(&path(), &value).unwrap();
    let bob: BTreeMap<String, u32> = docs_in(&storage, "bob").load(&path()).unwrap();
    assert!(bob.is_empty());
}

#[tokio::test]
async fn the_file_store_dispatches_to_documents_when_a_backend_is_pinned() {
    let storage = MemoryStorage::new();
    let docs = docs_in(&storage, "local");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("composio_identities.json");
    let mut value = BTreeMap::new();
    value.insert("gmail".to_string(), 7u32);
    // Pin on this thread for the awaits below (a current-thread runtime).
    super::TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(docs.clone()));
    super::super::file_store::save(&file, &value).await.unwrap();
    let loaded: BTreeMap<String, u32> = super::super::file_store::load(&file).await.unwrap();
    super::TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    assert_eq!(loaded, value);
    assert!(!file.exists());
}

#[tokio::test]
async fn the_file_path_still_works_with_no_backend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let mut value = BTreeMap::new();
    value.insert("gmail".to_string(), 3u32);
    super::super::file_store::save(&path, &value).await.unwrap();
    assert!(path.exists());
}
