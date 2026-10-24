use super::*;

#[tokio::test]
async fn boot_prepares_each_linked_crates_collections_on_host_scopes() {
    let backend: Arc<dyn StorageBackend> =
        Arc::new(tinystoragedrivers_testkit::MemoryBackend::new());

    let report = prepare_and_migrate(&backend).await.unwrap();
    let owners = super::super::schema::collection_owners().unwrap();
    let declared = owners
        .iter()
        .map(|owner| owner.collections.len())
        .sum::<usize>();
    assert_eq!(report.scopes, 2);
    assert_eq!(report.collections, declared * report.scopes);

    // Each owner's declarations are ensured again after an interrupted boot.
    let retry = prepare_and_migrate(&backend).await.unwrap();
    assert_eq!(retry, report);
}

#[tokio::test]
async fn tenant_scopes_use_each_linked_crates_collections() {
    let backend = tinystoragedrivers_testkit::MemoryBackend::new();
    let scope = super::super::scope::scope_for_profile("profile-a");
    let storage = backend.for_scope(&scope).unwrap();

    let prepared = prepare_scope(&storage).await.unwrap();
    let declared = super::super::schema::collection_owners()
        .unwrap()
        .iter()
        .map(|owner| owner.collections.len())
        .sum::<usize>();
    assert_eq!(prepared, declared);
}
