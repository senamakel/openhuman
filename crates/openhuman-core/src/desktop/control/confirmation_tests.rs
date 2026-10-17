use super::*;
use tinycomputer_bus::DesktopError;

fn record_save_prompt(id: &str) {
    record(
        "TextEdit",
        "make a note",
        "thread-a",
        &json!({
            "stop":"confirmation_required", "confirmation_id":id,
            "pending":{"operation":"CLICK", "reason":"May save a file",
                "target":{"ref_id":"@s1:e2", "name":"Save", "role":"button"}}
        }),
    );
}

#[tokio::test]
async fn confirmation_requires_trusted_one_use_decision_for_same_goal() {
    let _loopback = super::super::ops::test_loopback_guard();
    let id = uuid::Uuid::new_v4().to_string();
    record_save_prompt(&id);
    assert!(take_approved(&id, Some("thread-a")).is_err());
    let listed = pending();
    let item = listed
        .iter()
        .find(|item| item.confirmation_id == id)
        .unwrap();
    assert_eq!(item.operation, "CLICK");
    assert_eq!(item.target_name.as_deref(), Some("Save"));
    assert_eq!(item.action_summary, "CLICK button 'Save' in TextEdit");
    let config = Config::default();
    let approved = confirm(&config, &id, true).await.unwrap();
    assert_eq!(approved["approve"], true);
    assert_eq!(
        take_approved(&id, None).unwrap_err(),
        "desktop confirmation requires a threaded agent run"
    );
    assert_eq!(
        take_approved(&id, Some("thread-b")).unwrap_err(),
        "desktop confirmation does not match this thread"
    );
    assert_eq!(
        take_approved(&id, Some("thread-a")).unwrap(),
        ("TextEdit".to_owned(), "make a note".to_owned())
    );
    assert!(take_approved(&id, Some("thread-a")).is_err());
}

#[test]
fn unnamed_target_cannot_be_approved() {
    let _loopback = super::super::ops::test_loopback_guard();
    let id = uuid::Uuid::new_v4().to_string();
    record(
        "TextEdit",
        "test",
        "thread-a",
        &json!({
            "stop":"confirmation_required", "confirmation_id":id,
            "pending":{"operation":"CLICK", "target":{"ref_id":"@s1:e2", "role":"button"}}
        }),
    );
    assert!(!pending().iter().any(|item| item.confirmation_id == id));
}

#[tokio::test]
async fn failed_denial_keeps_the_handle_retryable_until_module_confirms_cancellation() {
    let _loopback = super::super::ops::test_loopback_guard();
    let id = uuid::Uuid::new_v4().to_string();
    record_save_prompt(&id);
    confirm_with(&id, true, |_app, _goal, _handle| async {
        panic!("approval must not call the desktop module")
    })
    .await
    .unwrap();

    let expected_id = id.clone();
    let transport_error = confirm_with(&id, false, move |app, goal, handle| async move {
        assert_eq!(
            (app.as_str(), goal.as_str(), handle.as_str()),
            ("TextEdit", "make a note", expected_id.as_str())
        );
        Err("transport lost".to_owned())
    })
    .await
    .unwrap_err();
    assert_eq!(transport_error, "transport lost");
    assert!(pending().iter().any(|item| item.confirmation_id == id));
    assert!(
        take_approved(&id, Some("thread-a")).is_err(),
        "a later denial must revoke earlier approval even when transport fails"
    );

    let module_error = confirm_with(&id, false, |_app, _goal, _handle| async {
        Ok(DesktopResponse::err(
            "run-goal",
            DesktopError::new("CANCEL_FAILED", "still waiting"),
        ))
    })
    .await
    .unwrap_err();
    assert_eq!(module_error, "still waiting");
    assert!(pending().iter().any(|item| item.confirmation_id == id));

    let wrong_stop = confirm_with(&id, false, |_app, _goal, _handle| async {
        Ok(DesktopResponse::ok(
            "run-goal",
            json!({"stop":"confirmation_required"}),
        ))
    })
    .await
    .unwrap_err();
    assert_eq!(wrong_stop, "desktop cancellation was not confirmed");
    assert!(pending().iter().any(|item| item.confirmation_id == id));

    let denied = confirm_with(&id, false, |_app, _goal, _handle| async {
        Ok(DesktopResponse::ok("run-goal", json!({"stop":"cancelled"})))
    })
    .await
    .unwrap();
    assert_eq!(denied["approve"], false);
    assert!(!pending().iter().any(|item| item.confirmation_id == id));
}

#[tokio::test]
async fn denial_in_flight_cannot_be_approved_or_sent_twice() {
    let _loopback = super::super::ops::test_loopback_guard();
    let id = uuid::Uuid::new_v4().to_string();
    record_save_prompt(&id);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        confirm_with(&task_id, false, |_app, _goal, _handle| async move {
            let _ = started_tx.send(());
            let _ = release_rx.await;
            Ok(DesktopResponse::ok("run-goal", json!({"stop":"cancelled"})))
        })
        .await
    });
    started_rx.await.unwrap();
    let reject = "desktop cancellation is already in progress";
    assert_eq!(
        confirm_with(&id, true, |_app, _goal, _handle| async {
            panic!("approval must not reach module while denial is in flight")
        })
        .await
        .unwrap_err(),
        reject
    );
    assert_eq!(
        confirm_with(&id, false, |_app, _goal, _handle| async {
            panic!("a second denial must not reach module")
        })
        .await
        .unwrap_err(),
        reject
    );
    assert_eq!(take_approved(&id, Some("thread-a")).unwrap_err(), reject);
    release_tx.send(()).unwrap();
    assert_eq!(task.await.unwrap().unwrap()["approve"], false);
    assert!(!pending().iter().any(|item| item.confirmation_id == id));
}
