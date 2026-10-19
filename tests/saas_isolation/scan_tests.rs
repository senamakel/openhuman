use super::*;
use openhuman_core::storage::{MemoryStorage, Precondition};
use serde_json::json;

use crate::canary;

#[test]
fn scoped_record_audit_counts_own_data_and_rejects_foreign_canaries() {
    let alice = openhuman_core::storage::scope_for_profile("alice");
    let mut own = [0; 3];
    let mut violations = Vec::new();

    record_scope_content(
        &alice,
        Some(0),
        "CANARY-alice-thread",
        &mut own,
        &mut violations,
    );
    assert_eq!(own, [1, 0, 0]);
    assert!(violations.is_empty());

    record_scope_content(
        &alice,
        Some(0),
        "CANARY-bob-thread",
        &mut own,
        &mut violations,
    );
    assert_eq!(own, [1, 0, 0]);
    assert!(violations[0].contains("bob's canary"));
}

#[test]
fn storage_audit_enumerates_scopes_records_identifiers_and_paged_streams() {
    let backend = MemoryStorage::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let report = runtime.block_on(async {
        for (index, user) in USERS.iter().enumerate() {
            let scoped = backend
                .for_scope(&openhuman_core::storage::scope_for_profile(user))
                .unwrap();
            let (id, collection, value) = if index == 0 {
                (
                    "CANARY-bob-document-id",
                    "CANARY-carol-collection",
                    canary(index, "own"),
                )
            } else {
                ("own", "canaries", canary(index, "own"))
            };
            scoped
                .documents()
                .put(collection, id, json!({"value": value}), Precondition::None)
                .await
                .unwrap();
        }

        let child = backend
            .for_scope(
                &openhuman_core::storage::scope_for_profile("alice")
                    .agent("worker")
                    .unwrap(),
            )
            .unwrap();
        for index in 0..129 {
            child
                .streams()
                .append(
                    "events",
                    json!({"value": if index == 128 { canary(1, "late-stream") } else { format!("entry-{index}") }}),
                )
                .await
                .unwrap();
        }

        let bob = backend
            .for_scope(&openhuman_core::storage::scope_for_profile("bob"))
            .unwrap();
        bob.blobs()
            .put("CANARY-alice-blob-key", b"plain body".to_vec(), None)
            .await
            .unwrap();

        let operator = backend.for_scope(&Scope::local()).unwrap();
        operator
            .documents()
            .put(
                "operator-data",
                "canary",
                json!({"value": canary(2, "operator")}),
                Precondition::None,
            )
            .await
            .unwrap();

        scan_storage(&backend).await.unwrap()
    });

    assert!(
        report.own.iter().all(|count| *count > 0),
        "{:?}",
        report.own
    );
    let findings = report.violations.join("\n");
    assert!(findings.contains("bob's canary"), "{findings}");
    assert!(findings.contains("carol's canary"), "{findings}");
    assert!(findings.contains("scope local"), "{findings}");
    assert!(findings.contains("CANARY-bob-document-id"), "{findings}");
    assert!(findings.contains("CANARY-carol-collection"), "{findings}");
    assert!(findings.contains("CANARY-alice-blob-key"), "{findings}");
    assert!(findings.contains("CANARY-bob-late-stream"), "{findings}");
    assert!(findings.contains("CANARY-carol-operator"), "{findings}");
}

#[test]
fn storage_database_exclusion_matches_only_exact_database_and_sidecars() {
    let db = Path::new("profile-store.sqlite");
    assert!(is_storage_db_file(db, db));
    assert!(is_storage_db_file(
        Path::new("profile-store.sqlite-wal"),
        db
    ));
    assert!(is_storage_db_file(
        Path::new("profile-store.sqlite-shm"),
        db
    ));
    assert!(!is_storage_db_file(
        Path::new("profile-store.sqlite.backup"),
        db
    ));
    assert!(!is_storage_db_file(Path::new("other.sqlite-wal"), db));
}
