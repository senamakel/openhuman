use super::*;
use crate::agent::orchestration::subagent_sessions::types::DurableSubagentStatus;
use crate::agent::orchestration::subagent_sessions::types::SubagentSessionStore;
use crate::storage::{MemoryStorage, Scope, StorageBackend};
use serde_json::json;

fn docs_in(storage: &MemoryStorage, scope: &str) -> Docs {
    Docs::over(&storage.for_scope(&Scope::new(scope).unwrap()).unwrap())
}

fn session(id: &str) -> DurableSubagentSession {
    serde_json::from_value(json!({
        "subagentSessionId": id,
        "parentSession": "parent",
        "parentThreadId": "thread",
        "workerThreadId": null,
        "agentId": "researcher",
        "displayName": null,
        "model": null,
        "sandboxMode": "workspace",
        "actionRoot": null,
        "taskKey": "task",
        "taskTitle": "Task",
        "currentTaskId": null,
        "status": "idle",
        "reusable": true,
        "latestHistory": null,
        "latestError": null,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "lastUsedAt": "2026-01-01T00:00:00Z",
    }))
    .unwrap()
}

#[test]
fn a_missing_list_loads_empty() {
    assert!(docs_in(&MemoryStorage::new(), "local")
        .load()
        .unwrap()
        .is_empty());
}

#[test]
fn save_upserts_sessions_and_keeps_the_others() {
    let docs = docs_in(&MemoryStorage::new(), "local");
    docs.save(&[session("a"), session("b")]).unwrap();
    let loaded = docs.load().unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].subagent_session_id, "a");
    assert_eq!(loaded[1].status, DurableSubagentStatus::Idle);
    // A save from a stale snapshot never removes a session it does not know.
    docs.save(&[session("c")]).unwrap();
    let ids: Vec<_> = docs
        .load()
        .unwrap()
        .into_iter()
        .map(|s| s.subagent_session_id)
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn scopes_do_not_see_each_others_sessions() {
    let storage = MemoryStorage::new();
    docs_in(&storage, "alice").save(&[session("a")]).unwrap();
    assert!(docs_in(&storage, "bob").load().unwrap().is_empty());
    assert_eq!(docs_in(&storage, "alice").load().unwrap().len(), 1);
}

#[test]
fn the_file_store_still_works_with_no_backend() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    assert!(store.load().unwrap().is_empty());
    store.save(&[session("a")]).unwrap();
    assert!(dir
        .path()
        .join(".openhuman/subagent_sessions.json")
        .exists());
    assert_eq!(store.load().unwrap().len(), 1);
}

#[test]
fn sessions_come_back_in_creation_order() {
    let docs = docs_in(&MemoryStorage::new(), "local");
    let mut early = session("z");
    early.created_at = "2026-01-01T00:00:00Z".into();
    let mut late = session("a");
    late.created_at = "2026-02-01T00:00:00Z".into();
    docs.save(&[late, early]).unwrap();
    let ids: Vec<_> = docs
        .load()
        .unwrap()
        .into_iter()
        .map(|s| s.subagent_session_id)
        .collect();
    assert_eq!(ids, ["z", "a"]);
}

#[test]
fn the_store_dispatches_to_documents_when_a_backend_is_pinned() {
    let storage = MemoryStorage::new();
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let docs = docs_in(&storage, "local");
    with_override(docs.clone(), || {
        assert!(store.load().unwrap().is_empty());
        store.save(&[session("a")]).unwrap();
        assert_eq!(store.load().unwrap().len(), 1);
    });
    assert_eq!(docs.load().unwrap().len(), 1);
    assert!(!dir
        .path()
        .join(".openhuman/subagent_sessions.json")
        .exists());
}
