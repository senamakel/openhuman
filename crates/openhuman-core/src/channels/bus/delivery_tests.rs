use super::*;
use crate::config::test_env::EnvVarGuard;

#[test]
fn outbound_body_has_stable_idempotency_key() {
    let body = json!({ "text": "hello" });
    let first = channel_message_body_with_idempotency("telegram", body.clone());
    let second = channel_message_body_with_idempotency("telegram", body);

    assert_eq!(first, second);
    assert!(first.get("idempotencyKey").is_some());
}

#[tokio::test]
async fn sender_methods_report_unavailable_without_a_hosted_session() {
    let _env_lock = crate::config::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let _workspace_env = EnvVarGuard::workspace_unlocked(workspace.path());
    let sender = BackendProgressiveSender;

    assert!(matches!(
        sender.send("telegram", "hello").await,
        Err(ProgressiveSendError::Unavailable)
    ));
    assert!(matches!(
        sender.edit("telegram", "message", "hello").await,
        Err(ProgressiveSendError::Unavailable)
    ));
    assert!(matches!(
        sender.delete("telegram", "message").await,
        Err(ProgressiveSendError::Unavailable)
    ));
    assert!(matches!(
        sender.typing("telegram").await,
        Err(ProgressiveSendError::Unavailable)
    ));
}
