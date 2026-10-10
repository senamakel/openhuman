use super::*;
use crate::storage::MemoryStorage;

fn meta(id: &str, created_at: u64) -> ProfileMeta {
    ProfileMeta {
        profile_id: ProfileId::parse(id).unwrap(),
        created_at,
        layout_version: super::super::types::LAYOUT_VERSION,
    }
}

async fn exercise(registry: &ProfileRegistry) {
    let alice = meta("alice", 1);
    assert!(registry.get(&alice.profile_id).await.unwrap().is_none());
    assert!(registry.create(&alice).await.unwrap());
    assert!(
        !registry.create(&meta("alice", 2)).await.unwrap(),
        "a second create keeps the first record"
    );
    assert_eq!(registry.get(&alice.profile_id).await.unwrap(), Some(alice));
    assert!(registry.create(&meta("bob", 3)).await.unwrap());
    let ids: Vec<_> = registry
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.profile_id.to_string())
        .collect();
    assert_eq!(ids, vec!["alice", "bob"]);
}

#[tokio::test]
async fn the_shared_registry_records_lists_and_forgets() {
    let storage = MemoryStorage::new();
    let registry = ProfileRegistry::cluster(&storage).unwrap();
    assert!(registry.is_shared());
    exercise(&registry).await;

    // A second node over the same backend sees the same profiles.
    let other = ProfileRegistry::cluster(&storage).unwrap();
    assert_eq!(other.list().await.unwrap().len(), 2);

    let alice = ProfileId::parse("alice").unwrap();
    assert!(registry.remove(&alice).await.unwrap());
    assert!(!registry.remove(&alice).await.unwrap());
    assert!(other.get(&alice).await.unwrap().is_none());
}

#[tokio::test]
async fn the_file_registry_keeps_profile_toml_beside_each_profile() {
    let tmp = tempfile::tempdir().unwrap();
    for id in ["alice", "bob"] {
        let layout = ProfileLayout::new(tmp.path(), ProfileId::parse(id).unwrap());
        std::fs::create_dir_all(&layout.dir).unwrap();
    }
    let registry = ProfileRegistry::files(tmp.path());
    assert!(!registry.is_shared());
    exercise(&registry).await;
    let alice = ProfileId::parse("alice").unwrap();
    assert!(ProfileLayout::new(tmp.path(), &alice).meta_path.is_file());

    // An unreadable record is skipped, not fatal; a stray directory is not
    // a profile.
    std::fs::write(ProfileLayout::new(tmp.path(), &alice).meta_path, "nope = [").unwrap();
    std::fs::create_dir_all(layout::users_dir(tmp.path()).join("Not An Id")).unwrap();
    let listed = registry.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].profile_id.as_str(), "bob");
}

#[tokio::test]
async fn an_empty_root_lists_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(ProfileRegistry::files(tmp.path().join("missing"))
        .list()
        .await
        .unwrap()
        .is_empty());
}
