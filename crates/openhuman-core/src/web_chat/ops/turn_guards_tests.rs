use super::*;

fn result(text: &str) -> WebChatTaskResult {
    WebChatTaskResult {
        full_response: text.to_string(),
        citations: Vec::new(),
        usage: None,
        workspace_dir: std::path::PathBuf::from("/tmp/turn-guards"),
        timing: None,
    }
}

/// The guarded turn sees the backstop it runs under, so the session turn can
/// hand the harness the deadline it must wind down before.
#[tokio::test]
async fn guarded_turn_sees_the_backstop_deadline() {
    let backstop = Duration::from_secs(900);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_in_turn = seen.clone();
    let res = drive_turn_with_deadline(Some(backstop), async move {
        *seen_in_turn.lock().unwrap() = crate::agent::turn_deadline::current();
        Ok(result("answer"))
    })
    .await
    .expect("turn completes");
    assert_eq!(res.full_response, "answer");
    let deadline = seen.lock().unwrap().expect("deadline scoped on the turn");
    assert_eq!(deadline.backstop(), backstop);
    assert_eq!(deadline.wind_down_after(), Duration::from_secs(780));
    assert!(deadline.wind_down_after() < deadline.hard_stop_after());
    assert!(deadline.hard_stop_after() < backstop);
}

#[tokio::test]
async fn disabled_backstop_scopes_no_deadline() {
    let res = drive_turn_with_deadline(None, async {
        assert!(crate::agent::turn_deadline::current().is_none());
        Ok(result("ok"))
    })
    .await;
    assert!(res.is_ok());
}

/// If the backstop fires anyway, the caller gets the typed `turn_timeout`
/// error, which is classified to user-facing copy, not silence.
#[tokio::test(start_paused = true)]
async fn backstop_firing_yields_a_classified_turn_timeout_error() {
    let err = drive_turn_with_deadline(Some(Duration::from_secs(900)), async {
        std::future::pending::<()>().await;
        Ok(result("never"))
    })
    .await
    .expect_err("backstop fires");
    assert!(super::super::super::web_errors::is_outer_backstop_timeout(
        &err
    ));
    let classified = super::super::super::web_errors::classify_inference_error(&err);
    assert_eq!(classified.error_type, "turn_timeout");
    assert!(!classified.message.trim().is_empty());
}
