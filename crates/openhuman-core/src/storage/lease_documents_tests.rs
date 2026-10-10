use super::*;

use std::time::Duration;

use crate::storage::MemoryStorage;

const TTL: Duration = Duration::from_millis(1_000);

fn node(storage: &MemoryStorage, id: &str) -> DocumentLeases {
    DocumentLeases::cluster(storage, id, Some(format!("http://{id}")), TTL).unwrap()
}

#[tokio::test]
async fn acquire_contend_expire_and_take_over() {
    let storage = MemoryStorage::new();
    let (a, b) = (node(&storage, "a"), node(&storage, "b"));

    let first = a.acquire("user-1", 0).await.unwrap();
    assert_eq!((first.epoch, first.previous_unclean), (1, false));
    assert_eq!(first.expires_at_ms, 1_000);

    match b.acquire("user-1", 500).await {
        Err(LeaseError::Held(record)) => {
            assert_eq!(record.owner, "a");
            assert_eq!(record.endpoint.as_deref(), Some("http://a"));
            assert_eq!(record.retry_after_ms(500), 500);
        }
        other => panic!("expected Held, got {other:?}"),
    }

    let taken = b.acquire("user-1", 1_000).await.unwrap();
    assert_eq!((taken.epoch, taken.previous_unclean), (2, true));
    assert_eq!(b.holder("user-1").await.unwrap().unwrap().owner, "b");

    assert!(matches!(
        a.renew(&first, 1_001).await,
        Err(LeaseError::Lost)
    ));
    assert!(matches!(a.release(first).await, Err(LeaseError::Lost)));
    assert_eq!(
        b.holder("user-1").await.unwrap().unwrap().owner,
        "b",
        "a stale release changes nothing"
    );
}

#[tokio::test]
async fn renew_extends_and_keeps_the_epoch() {
    let storage = MemoryStorage::new();
    let (a, b) = (node(&storage, "a"), node(&storage, "b"));
    let grant = a.acquire("k", 0).await.unwrap();
    let renewed = a.renew(&grant, 900).await.unwrap();
    assert_eq!(renewed.epoch, grant.epoch);
    assert_eq!(renewed.expires_at_ms, 1_900);
    assert!(renewed.version > grant.version);
    assert!(matches!(
        b.acquire("k", 1_500).await,
        Err(LeaseError::Held(_))
    ));
    assert!(
        matches!(a.renew(&grant, 950).await, Err(LeaseError::Lost)),
        "the superseded grant cannot renew"
    );
}

#[tokio::test]
async fn a_clean_release_hands_over_cleanly() {
    let storage = MemoryStorage::new();
    let (a, b) = (node(&storage, "a"), node(&storage, "b"));
    let grant = a.acquire("k", 0).await.unwrap();
    a.release(grant).await.unwrap();
    let record = a.holder("k").await.unwrap().unwrap();
    assert!(record.released && !record.is_live(1));
    let next = b.acquire("k", 1).await.unwrap();
    assert_eq!((next.epoch, next.previous_unclean), (2, false));
}

#[tokio::test]
async fn reacquire_is_reentrant_and_a_restart_is_unclean() {
    let storage = MemoryStorage::new();
    let a = node(&storage, "a");
    let grant = a.acquire("k", 0).await.unwrap();
    let again = a.acquire("k", 10).await.unwrap();
    assert_eq!((again.epoch, again.previous_unclean), (grant.epoch, false));
    assert!(matches!(a.renew(&grant, 20).await, Err(LeaseError::Lost)));

    let restarted = node(&storage, "a");
    let after = restarted.acquire("k", 20).await.unwrap();
    assert_eq!((after.epoch, after.previous_unclean), (2, true));
    assert!(matches!(a.renew(&again, 30).await, Err(LeaseError::Lost)));
}

#[tokio::test]
async fn bad_keys_and_corrupt_records_are_storage_errors() {
    let storage = MemoryStorage::new();
    let a = node(&storage, "a");
    assert!(matches!(
        a.acquire("../escape", 0).await,
        Err(LeaseError::Storage(_))
    ));
    assert!(matches!(a.holder("a/b").await, Err(LeaseError::Storage(_))));
    let scoped = storage
        .for_scope(&Scope::new(CLUSTER_SCOPE).unwrap())
        .unwrap();
    scoped
        .documents()
        .put(
            LEASE_COLLECTION,
            "k",
            serde_json::json!({ "owner": 7 }),
            Precondition::None,
        )
        .await
        .unwrap();
    assert!(matches!(
        a.acquire("k", 0).await,
        Err(LeaseError::Storage(_))
    ));
}

#[tokio::test]
async fn keys_are_independent() {
    let storage = MemoryStorage::new();
    let (a, b) = (node(&storage, "a"), node(&storage, "b"));
    a.acquire("one", 0).await.unwrap();
    let other = b.acquire("two", 0).await.unwrap();
    assert_eq!(other.epoch, 1);
    assert!(a.holder("three").await.unwrap().is_none());
}

/// Real threads, each its own runtime and node, race for one key on one
/// backend opened by URL: exactly one wins and the rest see it.
#[cfg(feature = "storage-sqlite")]
fn race_on(url: String, contenders: usize) {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(contenders));
    let handles: Vec<_> = (0..contenders)
        .map(|i| {
            let (barrier, url) = (std::sync::Arc::clone(&barrier), url.clone());
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async move {
                    let backend = crate::storage::open(&url).await.unwrap();
                    let leases =
                        DocumentLeases::cluster(backend.as_ref(), format!("n{i}"), None, TTL)
                            .unwrap();
                    leases.holder("race").await.unwrap();
                    barrier.wait();
                    leases.acquire("race", 0).await
                })
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let winners: Vec<_> = outcomes.iter().filter_map(|o| o.as_ref().ok()).collect();
    assert_eq!(winners.len(), 1, "{outcomes:?}");
    assert_eq!(winners[0].epoch, 1);
    for outcome in &outcomes {
        match outcome {
            Ok(_) => {}
            Err(LeaseError::Held(record)) => assert_eq!(record.epoch, 1),
            Err(other) => panic!("unexpected {other:?}"),
        }
    }
}

#[cfg(feature = "storage-sqlite")]
#[test]
fn sqlite_threads_racing_for_a_key_have_one_winner() {
    for round in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite:{}",
            dir.path().join(format!("leases-{round}.db")).display()
        );
        race_on(url, 8);
    }
}

#[tokio::test]
async fn renew_after_expiry_is_lost_even_without_takeover() {
    let storage = MemoryStorage::new();
    let a = node(&storage, "a");
    let grant = a.acquire("k", 0).await.unwrap();
    assert!(matches!(
        a.renew(&grant, 2_000).await,
        Err(LeaseError::Lost)
    ));
    assert!(a.renew(&grant, 999).await.is_ok());
}

#[tokio::test]
async fn another_instances_grant_cannot_renew_or_release() {
    let storage = MemoryStorage::new();
    let (a, b) = (node(&storage, "a"), node(&storage, "b"));
    let grant = a.acquire("k", 0).await.unwrap();
    assert!(matches!(b.renew(&grant, 10).await, Err(LeaseError::Lost)));
    assert!(matches!(
        b.release(grant.clone()).await,
        Err(LeaseError::Lost)
    ));
    assert_eq!(a.holder("k").await.unwrap().unwrap().owner, "a");
    assert!(a.renew(&grant, 10).await.is_ok());
}

#[tokio::test]
async fn sub_millisecond_ttl_does_not_expire_at_acquisition() {
    let storage = MemoryStorage::new();
    let a = DocumentLeases::cluster(&storage, "a", None, Duration::ZERO).unwrap();
    let b = node(&storage, "b");
    let grant = a.acquire("k", 5).await.unwrap();
    assert!(grant.expires_at_ms > 5);
    assert!(matches!(b.acquire("k", 5).await, Err(LeaseError::Held(_))));
}
