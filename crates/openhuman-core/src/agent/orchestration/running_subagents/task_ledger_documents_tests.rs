use super::*;
use crate::storage::{MemoryStorage, Scope, StorageBackend};
use tinyagents_tasks::{OrchestrationTaskKind, OrchestrationTaskStatus};

fn repo_in(storage: &MemoryStorage, scope: &str) -> Repo {
    let scoped = storage.for_scope(&Scope::new(scope).unwrap()).unwrap();
    Repo::over(&scoped, DOMAIN, collections)
}

fn spec(id: &str) -> OrchestrationTaskSpec {
    OrchestrationTaskSpec::new(id, OrchestrationTaskKind::SubAgent { agent: "a".into() })
}

#[test]
fn transitions_persist_and_replay_on_reopen() {
    let storage = MemoryStorage::new();
    let store = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    store.insert(spec("t1")).unwrap();
    store.mark_running(&TaskId::from("t1")).unwrap();
    store.insert(spec("t2")).unwrap();
    store.mark_running(&TaskId::from("t2")).unwrap();
    store.fail(&TaskId::from("t2"), "boom".into()).unwrap();

    let reopened = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    let t1 = reopened.get(&TaskId::from("t1")).unwrap();
    assert_eq!(t1.status, OrchestrationTaskStatus::Running);
    let t2 = reopened.get(&TaskId::from("t2")).unwrap();
    assert_eq!(t2.status, OrchestrationTaskStatus::Failed);
    assert_eq!(t2.error.as_deref(), Some("boom"));
    assert_eq!(reopened.list(OrchestrationTaskFilter::default()).len(), 2);
    // pending -> running -> failed
    assert_eq!(reopened.history(&TaskId::from("t2")).len(), 3);
}

#[test]
fn cancel_requests_persist() {
    let storage = MemoryStorage::new();
    let store = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    store.insert(spec("t1")).unwrap();
    store.mark_running(&TaskId::from("t1")).unwrap();
    store.request_cancel(&TaskId::from("t1")).unwrap();
    let reopened = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    assert_eq!(
        reopened.get(&TaskId::from("t1")).unwrap().status,
        OrchestrationTaskStatus::CancelRequested
    );
}

#[test]
fn scopes_do_not_share_a_ledger() {
    let storage = MemoryStorage::new();
    let alice = DocumentTaskStore::open(repo_in(&storage, "alice")).unwrap();
    alice.insert(spec("t1")).unwrap();
    let bob = DocumentTaskStore::open(repo_in(&storage, "bob")).unwrap();
    assert!(bob.list(OrchestrationTaskFilter::default()).is_empty());
    assert!(bob.get(&TaskId::from("t1")).is_none());
    let alice_again = DocumentTaskStore::open(repo_in(&storage, "alice")).unwrap();
    assert!(alice_again.get(&TaskId::from("t1")).is_some());
}

fn id(raw: &str) -> TaskId {
    TaskId::from(raw)
}

#[test]
fn every_transition_is_written_through() {
    let storage = MemoryStorage::new();
    let store = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    for name in ["a", "b", "c", "d", "e", "f"] {
        store.insert(spec(name)).unwrap();
    }
    store.mark_awaiting(&id("a")).unwrap();
    store
        .mark_awaiting_with_question(&id("b"), "which one?".into())
        .unwrap();
    store.mark_running(&id("c")).unwrap();
    store
        .complete(&id("c"), OrchestrationTaskResult::text("done"))
        .unwrap();
    store.timeout(&id("d"), "too slow".into()).unwrap();
    store.request_cancel(&id("e")).unwrap();
    store.mark_cancelled(&id("e")).unwrap();
    store.kill(&id("f")).unwrap();
    store.set_timeout_ms(&id("a"), 5_000).unwrap();

    let reopened = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    let status = |name: &str| reopened.get(&id(name)).unwrap().status;
    assert_eq!(status("a"), OrchestrationTaskStatus::Awaiting);
    assert_eq!(status("b"), OrchestrationTaskStatus::Awaiting);
    assert_eq!(
        reopened.get(&id("b")).unwrap().error.as_deref(),
        Some("which one?")
    );
    assert_eq!(status("c"), OrchestrationTaskStatus::Completed);
    assert_eq!(status("d"), OrchestrationTaskStatus::TimedOut);
    assert_eq!(status("e"), OrchestrationTaskStatus::Cancelled);
    assert_eq!(status("f"), OrchestrationTaskStatus::Abandoned);
    assert_eq!(reopened.get(&id("a")).unwrap().spec.timeout_ms, Some(5_000));
}

#[test]
fn an_invalid_transition_is_rejected_and_not_written() {
    let storage = MemoryStorage::new();
    let store = DocumentTaskStore::open(repo_in(&storage, "local")).unwrap();
    store.insert(spec("a")).unwrap();
    assert!(store.mark_cancelled(&id("a")).is_err());
    assert!(store.mark_running(&id("missing")).is_err());
    assert_eq!(store.history(&id("a")).len(), 1);
}

#[test]
fn a_malformed_document_is_skipped_on_open() {
    let storage = MemoryStorage::new();
    let repo = repo_in(&storage, "local");
    repo.run(|docs| async move {
        docs.put(
            TASKS,
            "bad",
            serde_json::json!({ "record": 1 }),
            Precondition::None,
        )
        .await
        .map(|_| ())
    })
    .unwrap();
    let store = DocumentTaskStore::open(repo.clone()).unwrap();
    assert!(store.list(OrchestrationTaskFilter::default()).is_empty());
    let boxed = open_or_memory(repo);
    assert!(boxed.list(OrchestrationTaskFilter::default()).is_empty());
}

#[test]
fn the_ledger_dispatches_to_documents_when_a_backend_is_pinned() {
    let storage = MemoryStorage::new();
    let workspace = tempfile::tempdir().unwrap();
    let repo = repo_in(&storage, "local");
    with_override(repo.clone(), || {
        let store = super::super::task_ledger::task_store_for_workspace(workspace.path());
        store.insert(spec("pinned")).unwrap();
    });
    // The record is in the documents, and no JSONL file was made.
    let reopened = DocumentTaskStore::open(repo).unwrap();
    assert!(reopened.get(&id("pinned")).is_some());
    assert!(!workspace
        .path()
        .join(".openhuman/orchestration_tasks.jsonl")
        .exists());
}

#[test]
fn the_ledger_uses_the_jsonl_file_with_no_backend() {
    let workspace = tempfile::tempdir().unwrap();
    let store = super::super::task_ledger::task_store_for_workspace(workspace.path());
    store.insert(spec("plain")).unwrap();
    assert!(workspace
        .path()
        .join(".openhuman/orchestration_tasks.jsonl")
        .exists());
}
