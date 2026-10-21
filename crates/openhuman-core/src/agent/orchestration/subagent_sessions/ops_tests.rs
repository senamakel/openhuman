use super::*;
use crate::agent::orchestration::subagent_sessions::types::SubagentSessionUpsert;
use tinyagents_orchestration::subagent::{FinishedOutcome, SubAgentJobRegistry, SubagentTaskKey};

fn selector(task_key: &str) -> SubagentSessionSelector {
    SubagentSessionSelector {
        parent_session: "parent-a".into(),
        parent_thread_id: Some("thread-a".into()),
        agent_id: "researcher".into(),
        model: Some("oh-1".into()),
        sandbox_mode: "read_only".into(),
        action_root: Some("/tmp/work".into()),
        task_key: task_key.into(),
    }
}

#[test]
fn normalize_task_key_is_deterministic() {
    assert_eq!(
        normalize_task_key("  Review: GitHub PR #123!! "),
        "review-github-pr-123"
    );
    assert_eq!(normalize_task_key("   "), "untitled-task");
}

#[test]
fn normalize_task_key_preserves_non_latin_words() {
    assert_ne!(normalize_task_key("研究 caching"), "untitled-task");
    assert_ne!(
        normalize_task_key("研究 caching"),
        normalize_task_key("調査 caching")
    );
}

#[test]
fn normalize_task_key_hashes_empty_or_long_colliding_slugs() {
    assert_ne!(normalize_task_key("🙂🙂🙂"), "untitled-task");
    let prefix = "research ".repeat(40);
    let first = normalize_task_key(&(prefix.clone() + "alpha"));
    let second = normalize_task_key(&(prefix + "beta"));
    assert!(first.len() <= 96, "{first}");
    assert!(second.len() <= 96, "{second}");
    assert_ne!(first, second);
}

#[test]
fn compatible_session_reuses_and_incompatible_shape_spawns_new() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let upsert = SubagentSessionUpsert {
        selector: selector("same-task"),
        display_name: Some("Researcher".into()),
        task_title: "Same task".into(),
        worker_thread_id: Some("worker-1".into()),
        task_id: "sub-1".into(),
        lifecycle_key: None,
    };
    let session = upsert_running(&store, upsert, None).unwrap();
    mark_finished(
        &store,
        &session.subagent_session_id,
        "sub-1",
        &SubagentRunStatus::Completed,
        vec![TranscriptMessage::user("done")],
    )
    .unwrap();

    let reusable = find_reusable(&store, &selector("same-task"))
        .unwrap()
        .expect("same selector reuses");
    assert_eq!(reusable.subagent_session_id, session.subagent_session_id);

    let mut different = selector("same-task");
    different.action_root = Some("/tmp/other".into());
    assert!(find_reusable(&store, &different).unwrap().is_none());
}

#[test]
fn stale_completion_cannot_overwrite_reused_or_closed_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let first = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("same-task"),
            display_name: None,
            task_title: "First run".into(),
            worker_thread_id: None,
            task_id: "first-task".into(),
            lifecycle_key: None,
        },
        None,
    )
    .unwrap();
    let second = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("same-task"),
            display_name: None,
            task_title: "Second run".into(),
            worker_thread_id: None,
            task_id: "second-task".into(),
            lifecycle_key: None,
        },
        Some(&first),
    )
    .unwrap();
    assert_eq!(first.subagent_session_id, second.subagent_session_id);

    assert!(mark_finished(
        &store,
        &second.subagent_session_id,
        "first-task",
        &SubagentRunStatus::Completed,
        vec![TranscriptMessage::user("stale")],
    )
    .is_err());
    let current = store.load().unwrap().remove(0);
    assert_eq!(current.current_task_id.as_deref(), Some("second-task"));
    assert_eq!(current.status, DurableSubagentStatus::Running);
    assert!(current.latest_history.is_none());
    assert!(mark_failed(
        &store,
        &second.subagent_session_id,
        "first-task",
        "stale failure".into(),
    )
    .is_err());
    assert!(store.load().unwrap()[0].latest_error.is_none());

    close(&store, &second.subagent_session_id).unwrap();
    assert!(mark_finished(
        &store,
        &second.subagent_session_id,
        "second-task",
        &SubagentRunStatus::Completed,
        vec![TranscriptMessage::user("late after close")],
    )
    .is_err());
    assert!(mark_failed(
        &store,
        &second.subagent_session_id,
        "second-task",
        "late failure".into(),
    )
    .is_err());
    let closed = store.load().unwrap().remove(0);
    assert_eq!(closed.status, DurableSubagentStatus::Closed);
    assert!(closed.latest_history.is_none());
}

#[test]
fn driver_task_cancel_reports_legacy_failure_and_refuses_ambiguous_ids() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let jobs = SubAgentJobRegistry::default();
    let failed = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("failed"),
            display_name: None,
            task_title: "Failed child".into(),
            worker_thread_id: None,
            task_id: "finished-failed".into(),
            lifecycle_key: None,
        },
        None,
    )
    .unwrap();
    mark_failed(
        &store,
        &failed.subagent_session_id,
        "finished-failed",
        "provider failed".into(),
    )
    .unwrap();
    let selected = cancel_driver_job_by_task(&store, dir.path(), &jobs, "finished-failed", None)
        .unwrap()
        .expect("failed public task remains reportable");
    assert_eq!(selected.already_finished, Some(FinishedOutcome::Failed));

    let ambiguous = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("ambiguous"),
            display_name: None,
            task_title: "Ambiguous child".into(),
            worker_thread_id: None,
            task_id: "same-public-id".into(),
            lifecycle_key: Some(SubagentTaskKey {
                root_run_id: "root-a".into(),
                parent_run_id: "parent-a".into(),
                thread_id: Some("thread-a".into()),
                task_id: "same-public-id".into(),
            }),
        },
        None,
    )
    .unwrap();
    let second = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("ambiguous-2"),
            display_name: None,
            task_title: "Ambiguous child 2".into(),
            worker_thread_id: None,
            task_id: "same-public-id".into(),
            lifecycle_key: Some(SubagentTaskKey {
                root_run_id: "root-b".into(),
                parent_run_id: "parent-b".into(),
                thread_id: Some("thread-a".into()),
                task_id: "same-public-id".into(),
            }),
        },
        None,
    )
    .unwrap();
    assert_ne!(ambiguous.subagent_session_id, second.subagent_session_id);
    assert!(
        cancel_driver_job_by_task(&store, dir.path(), &jobs, "same-public-id", None)
            .unwrap()
            .is_none()
    );
    assert!(store
        .load()
        .unwrap()
        .iter()
        .filter(|row| row.current_task_id.as_deref() == Some("same-public-id"))
        .all(|row| row.status == DurableSubagentStatus::Running));
}

#[test]
fn driver_task_cancel_propagates_session_store_load_errors() {
    let dir = tempfile::tempdir().unwrap();
    let legacy_dir = dir.path().join(".openhuman");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(legacy_dir.join("subagent_sessions.json"), b"not valid json").unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());

    let error = cancel_driver_job_by_task(
        &store,
        dir.path(),
        &SubAgentJobRegistry::default(),
        "missing-task",
        None,
    )
    .expect_err("corrupt session data is not an unknown task");

    assert!(error.contains("failed to load subagent sessions for cancellation"));
    assert!(error.contains("failed to parse legacy subagent sessions"));
}

#[test]
fn cancellation_reason_is_written_only_for_the_observed_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let key = SubagentTaskKey {
        root_run_id: "root-a".into(),
        parent_run_id: "parent-a".into(),
        thread_id: Some("thread-a".into()),
        task_id: "cancel-reason-task".into(),
    };
    let session = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("reason"),
            display_name: None,
            task_title: "Reason".into(),
            worker_thread_id: None,
            task_id: key.task_id.clone(),
            lifecycle_key: Some(key.clone()),
        },
        None,
    )
    .unwrap();
    assert!(record_cancel_reason(&store, &session, "finish the current step").unwrap());
    assert_eq!(
        store.load().unwrap()[0].latest_error.as_deref(),
        Some("finish the current step")
    );

    let mut stale = session;
    stale.lifecycle_key = Some(SubagentTaskKey {
        root_run_id: "root-b".into(),
        ..key
    });
    assert!(!record_cancel_reason(&store, &stale, "stale reason").unwrap());
    assert_eq!(
        store.load().unwrap()[0].latest_error.as_deref(),
        Some("finish the current step")
    );
}

#[test]
fn paused_driver_cancel_marks_only_the_observed_session_failed() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let lifecycle_key = SubagentTaskKey {
        root_run_id: "root-a".into(),
        parent_run_id: "parent-a".into(),
        thread_id: Some("thread-a".into()),
        task_id: "paused-cancel-task".into(),
    };
    let session = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("paused-cancel"),
            display_name: None,
            task_title: "Paused child".into(),
            worker_thread_id: None,
            task_id: lifecycle_key.task_id.clone(),
            lifecycle_key: Some(lifecycle_key.clone()),
        },
        None,
    )
    .unwrap();
    mark_finished(
        &store,
        &session.subagent_session_id,
        &lifecycle_key.task_id,
        &SubagentRunStatus::AwaitingUser {
            question: "Need input".into(),
            options: None,
            checkpoint: None,
        },
        vec![TranscriptMessage::user("Need input")],
    )
    .unwrap();

    let selected = DriverTaskCancellation {
        task_id: lifecycle_key.task_id.clone(),
        agent_id: session.agent_id.clone(),
        parent_session: session.parent_session.clone(),
        subagent_session_id: session.subagent_session_id.clone(),
        workspace_dir: dir.path().to_path_buf(),
        parent_thread_id: session.parent_thread_id.clone(),
        already_finished: None,
        was_paused: true,
        lifecycle_key: Some(lifecycle_key.clone()),
    };
    assert!(mark_driver_pause_cancelled(&store, &selected).unwrap());
    assert!(!mark_driver_pause_cancelled(&store, &selected).unwrap());
    let cancelled = store.load().unwrap().remove(0);
    assert_eq!(cancelled.status, DurableSubagentStatus::Failed);
    assert_eq!(
        cancelled.current_task_id.as_deref(),
        Some("paused-cancel-task")
    );
    assert_eq!(cancelled.latest_history.as_ref().unwrap().len(), 1);

    let mut stale = selected;
    stale.lifecycle_key = Some(SubagentTaskKey {
        root_run_id: "root-b".into(),
        ..lifecycle_key
    });
    assert!(!mark_driver_pause_cancelled(&store, &stale).unwrap());
}

#[test]
fn stale_close_and_reuse_snapshots_cannot_change_a_newer_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let first_key = tinyagents_orchestration::subagent::SubagentTaskKey {
        root_run_id: "root-a".into(),
        parent_run_id: "parent-a".into(),
        thread_id: Some("thread-a".into()),
        task_id: "same-public-task".into(),
    };
    let second_key = tinyagents_orchestration::subagent::SubagentTaskKey {
        root_run_id: "root-b".into(),
        parent_run_id: "parent-b".into(),
        thread_id: Some("thread-a".into()),
        task_id: "same-public-task".into(),
    };
    let first = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("same-task"),
            display_name: None,
            task_title: "First run".into(),
            worker_thread_id: None,
            task_id: "same-public-task".into(),
            lifecycle_key: Some(first_key.clone()),
        },
        None,
    )
    .unwrap();
    let second = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("same-task"),
            display_name: None,
            task_title: "Second run".into(),
            worker_thread_id: None,
            task_id: "same-public-task".into(),
            lifecycle_key: Some(second_key.clone()),
        },
        Some(&first),
    )
    .unwrap();

    assert!(
        !close_if_current(&store, &first).unwrap(),
        "a close based on the stale first snapshot must lose the race"
    );
    let current = list_for_parent(&store, "parent-a", Some("thread-a"))
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(current.current_task_id.as_deref(), Some("same-public-task"));
    assert_eq!(current.lifecycle_key, Some(second_key));
    assert_eq!(current.status, DurableSubagentStatus::Running);
    assert!(
        current.reusable,
        "stale close must preserve replacement reuse"
    );
    assert!(
        upsert_running(
            &store,
            SubagentSessionUpsert {
                selector: selector("same-task"),
                display_name: None,
                task_title: "Stale retry".into(),
                worker_thread_id: None,
                task_id: "stale-task".into(),
                lifecycle_key: None,
            },
            Some(&first),
        )
        .is_err(),
        "a stale reuse snapshot must not replace the newer lifecycle"
    );

    assert!(close_if_current(&store, &second).unwrap());
    assert!(
        close_if_current(&store, &second).unwrap(),
        "closing the same already-closed durable session remains idempotent"
    );
    assert!(
        upsert_running(
            &store,
            SubagentSessionUpsert {
                selector: selector("same-task"),
                display_name: None,
                task_title: "Late retry".into(),
                worker_thread_id: None,
                task_id: "late-task".into(),
                lifecycle_key: None,
            },
            Some(&second),
        )
        .is_err(),
        "a stale retry must not reopen a session closed after observation"
    );
}

#[test]
fn upsert_persists_the_full_lifecycle_key_without_collapsing_to_task_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let key = tinyagents_orchestration::subagent::SubagentTaskKey {
        root_run_id: "root-run".into(),
        parent_run_id: "turn-run".into(),
        thread_id: Some("thread-a".into()),
        task_id: "sub-1".into(),
    };
    let session = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("durable-task"),
            display_name: None,
            task_title: "Task".into(),
            worker_thread_id: None,
            task_id: "sub-1".into(),
            lifecycle_key: Some(key.clone()),
        },
        None,
    )
    .unwrap();

    let loaded = store
        .load()
        .unwrap()
        .into_iter()
        .find(|row| row.subagent_session_id == session.subagent_session_id)
        .unwrap();
    assert_eq!(loaded.current_task_id.as_deref(), Some("sub-1"));
    assert_eq!(loaded.lifecycle_key, Some(key.clone()));
    let resolved = resolve_lifecycle_key(
        &store,
        &session.subagent_session_id,
        "parent-a",
        Some("thread-a"),
        dir.path(),
    )
    .unwrap();
    assert_eq!(resolved, key);
    assert!(resolve_lifecycle_key(
        &store,
        &session.subagent_session_id,
        "another-parent",
        Some("thread-a"),
        dir.path(),
    )
    .is_err());
}

#[test]
fn closed_session_is_not_reusable() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let session = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("task"),
            display_name: None,
            task_title: "Task".into(),
            worker_thread_id: None,
            task_id: "sub-1".into(),
            lifecycle_key: None,
        },
        None,
    )
    .unwrap();

    assert!(close(&store, &session.subagent_session_id).unwrap());
    let reusable = find_reusable(&store, &selector("task")).unwrap();
    assert!(
        reusable.is_none(),
        "closed session {} matched reusable record {:?}",
        session.subagent_session_id,
        reusable.map(|record| (record.subagent_session_id, record.status, record.reusable))
    );
}

#[test]
fn an_equal_millisecond_live_close_persists_and_a_stale_snapshot_cannot_reopen_it() {
    use crate::agent::orchestration::subagent_sessions::store_documents::{with_override, Docs};
    use crate::storage::{MemoryStorage, Scope, StorageBackend};

    let storage = MemoryStorage::new();
    let docs = Docs::over(&storage.for_scope(&Scope::new("local").unwrap()).unwrap());
    let store = SubagentSessionStore::new(std::env::temp_dir());
    let original = with_override(docs.clone(), || {
        upsert_running(
            &store,
            SubagentSessionUpsert {
                selector: selector("equal-ms-close"),
                display_name: None,
                task_title: "Task".into(),
                worker_thread_id: None,
                task_id: "task-id".into(),
                lifecycle_key: None,
            },
            None,
        )
        .unwrap()
    });
    let stale_snapshot = docs.load().unwrap();

    with_override(docs.clone(), || {
        store
            .update_session(&original.subagent_session_id, |current, _now| {
                current.status = DurableSubagentStatus::Closed;
                current.reusable = false;
                // Leave updated_at untouched to force an equal `_ms` live
                // transition deterministically, without a sleep or clock shim.
            })
            .unwrap()
            .expect("the session is updated");

        // A whole-list snapshot at the same timestamp must not reopen it.
        store.save(&stale_snapshot).unwrap();
    });

    let closed = docs
        .load()
        .unwrap()
        .into_iter()
        .find(|record| record.subagent_session_id == original.subagent_session_id)
        .expect("session remains stored");
    assert_eq!(closed.status, DurableSubagentStatus::Closed);
    assert!(!closed.reusable);
}

#[test]
fn list_for_parent_without_thread_id_does_not_filter_by_thread() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let first = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: selector("first"),
            display_name: None,
            task_title: "First".into(),
            worker_thread_id: None,
            task_id: "sub-1".into(),
            lifecycle_key: None,
        },
        None,
    )
    .unwrap();

    let mut second_selector = selector("second");
    second_selector.parent_thread_id = Some("thread-b".into());
    let second = upsert_running(
        &store,
        SubagentSessionUpsert {
            selector: second_selector,
            display_name: None,
            task_title: "Second".into(),
            worker_thread_id: None,
            task_id: "sub-2".into(),
            lifecycle_key: None,
        },
        None,
    )
    .unwrap();

    let all = list_for_parent(&store, "parent-a", None).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .any(|session| session.subagent_session_id == first.subagent_session_id));
    assert!(all
        .iter()
        .any(|session| session.subagent_session_id == second.subagent_session_id));

    let thread_a = list_for_parent(&store, "parent-a", Some("thread-a")).unwrap();
    assert_eq!(thread_a.len(), 1);
    assert_eq!(thread_a[0].subagent_session_id, first.subagent_session_id);
}

#[test]
fn missing_session_updates_return_errors() {
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    assert!(mark_finished(
        &store,
        "missing",
        "sub-1",
        &SubagentRunStatus::Completed,
        vec![]
    )
    .is_err());
    assert!(mark_failed(&store, "missing", "sub-1", "boom".into()).is_err());
    assert!(!close(&store, "missing").unwrap());
}

#[test]
fn legacy_session_with_toolkit_key_still_loads_and_reuses() {
    // Sessions persisted before the `toolkit` spawn argument was retired
    // carry a `toolkit` key. It must be ignored on load, and it must no
    // longer take part in reuse matching.
    let dir = tempfile::tempdir().unwrap();
    let store = SubagentSessionStore::new(dir.path().to_path_buf());
    let legacy = serde_json::json!([{
        "subagentSessionId": "subsess-legacy",
        "parentSession": "parent-a",
        "parentThreadId": "thread-a",
        "workerThreadId": null,
        "agentId": "researcher",
        "displayName": null,
        "toolkit": "gmail",
        "model": "oh-1",
        "sandboxMode": "read_only",
        "actionRoot": "/tmp/work",
        "taskKey": "legacy-task",
        "taskTitle": "Legacy task",
        "currentTaskId": null,
        "status": "idle",
        "reusable": true,
        "latestHistory": null,
        "latestError": null,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "lastUsedAt": "2026-01-01T00:00:00Z"
    }]);
    let parsed: Vec<DurableSubagentSession> =
        serde_json::from_value(legacy).expect("legacy toolkit key is ignored");
    store.save(&parsed).unwrap();

    let sessions = store.load().expect("legacy toolkit key is ignored");
    assert_eq!(sessions.len(), 1);
    let reused = find_reusable(&store, &selector("legacy-task"))
        .unwrap()
        .expect("legacy session reuses without a toolkit match");
    assert_eq!(reused.subagent_session_id, "subsess-legacy");
}
