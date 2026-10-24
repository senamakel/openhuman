use super::*;
#[cfg(feature = "storage-sqlite")]
use crate::storage::test_support::sqlite_backend;
#[cfg(feature = "storage-sqlite")]
use crate::storage::Scope;
use serde_json::json;
#[cfg(feature = "storage-sqlite")]
use std::sync::Arc;
use tinystoragedrivers::Meta;
#[cfg(feature = "storage-sqlite")]
use tinystoragedrivers::Precondition;

#[test]
fn a_registry_row_loses_its_seconds_field_and_keeps_its_creation_time() {
    let out = registry_v2(vec![SourceRecord {
        id: "alice".into(),
        body: json!({ "profile_id": "alice", "created_at": 1_700_000_000_u64, "layout_version": 2 }),
        meta: Meta::unknown(),
    }])
    .unwrap();
    assert_eq!(out[0].collection, REGISTRY_COLLECTION);
    assert_eq!(out[0].id, "alice");
    assert_eq!(
        out[0].body,
        json!({ "profile_id": "alice", "layout_version": 2 })
    );
    assert_eq!(out[0].created_ms, Some(1_700_000_000_000));
}

#[tokio::test]
#[cfg(feature = "storage-sqlite")]
async fn migrate_cluster_moves_the_v1_registry_into_the_v2_collection() {
    let fixture = sqlite_backend().unwrap();
    let backend = Arc::clone(&fixture.backend);
    let scoped = backend.for_scope(&Scope::cluster()).unwrap();
    let docs = scoped.documents();
    docs.ensure_collection(&CollectionSpec::new("profiles"))
        .await
        .unwrap();
    docs.put(
        "profiles",
        "alice",
        json!({ "profile_id": "alice", "created_at": 5, "layout_version": 2 }),
        Precondition::Absent,
    )
    .await
    .unwrap();
    migrate_cluster(&scoped).await.unwrap();
    migrate_cluster(&scoped).await.unwrap();
    let moved = docs
        .get(REGISTRY_COLLECTION, "alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        moved.doc,
        json!({ "profile_id": "alice", "layout_version": 2 })
    );
    assert_eq!(moved.meta.created_ms, 5000);
}

#[test]
fn the_schema_composes_under_its_owner() {
    assert_eq!(
        collection_specs(),
        vec![CollectionSpec::new(REGISTRY_COLLECTION)]
    );
}

#[tokio::test]
#[cfg(feature = "storage-sqlite")]
async fn migrate_cluster_moves_v1_leases_into_tsd_leases_and_keeps_their_epoch() {
    use crate::storage::lease::{DocumentLeases, LeaseStore, LEASE_OWNER};
    let fixture = sqlite_backend().unwrap();
    let backend = Arc::clone(&fixture.backend);
    let scoped = backend.for_scope(&Scope::cluster()).unwrap();
    let docs = scoped.documents();
    docs.ensure_collection(&CollectionSpec::new("leases"))
        .await
        .unwrap();
    docs.put(
        "leases",
        "alice",
        json!({ "owner": "node-a", "endpoint": "http://a", "epoch": 4,
                "expires_at_ms": 5_000, "released": false }),
        Precondition::Absent,
    )
    .await
    .unwrap();
    docs.put(
        "leases",
        "bob",
        json!({ "owner": "node-a", "endpoint": null, "epoch": 2,
                "expires_at_ms": 1_000, "released": true }),
        Precondition::Absent,
    )
    .await
    .unwrap();
    migrate_cluster(&scoped).await.unwrap();
    migrate_cluster(&scoped).await.unwrap();
    assert!(docs.get("leases", "alice").await.unwrap().is_none());
    assert!(docs.get("leases", "bob").await.unwrap().is_none());
    let id = crate::storage::schema_support::key_of_parts([LEASE_OWNER, "alice"]).unwrap();
    let moved = docs.get("tsd.leases", id.as_str()).await.unwrap().unwrap();
    assert_eq!(
        moved.doc,
        json!({ "holder": "node-a", "epoch": 4, "expires_ms": 5_000, "endpoint": "http://a" })
    );

    // The host's lease store reads the moved records: alice is still held,
    // bob's released record is taken over cleanly at the next epoch.
    let b = DocumentLeases::cluster(
        backend.as_ref(),
        "node-b",
        None,
        std::time::Duration::from_secs(1),
    )
    .unwrap();
    let held = b.holder("alice").await.unwrap().unwrap();
    assert_eq!((held.owner.as_str(), held.epoch), ("node-a", 4));
    assert!(b.acquire("alice", 1_000).await.is_err());
    let taken = b.acquire("bob", 2_000).await.unwrap();
    assert_eq!((taken.epoch, taken.previous_unclean), (3, false));
    let after = b.acquire("alice", 5_000).await.unwrap();
    assert_eq!((after.epoch, after.previous_unclean), (5, true));
}
