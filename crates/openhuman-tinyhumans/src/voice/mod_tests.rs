use super::*;

#[test]
fn backend_authentication_categories_survive_provider_dispatch() {
    for (error, expected) in [
        (
            BackendApiError::Unauthorized {
                method: "POST".into(),
                path: "/openai/v1/audio/speech".into(),
            },
            "session_expired",
        ),
        (
            BackendApiError::ApiKeyRejected {
                method: "GET".into(),
                path: "/voice-agent/get-signed-url".into(),
            },
            "api_key_rejected",
        ),
        (
            BackendApiError::BackendUnavailable {
                method: "POST".into(),
                path: "/agent-integrations/gemini/live/sessions".into(),
            },
            "unavailable",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(classify_backend_error(anyhow::Error::new(error), "")).unwrap()
                ["kind"],
            expected
        );
    }
    let error = classify_backend_error(anyhow::anyhow!("connection refused"), "");
    assert!(matches!(error, VoiceError::Transport { .. }));
    assert_eq!(error.to_string(), "connection refused");
}

#[test]
fn backend_failure_details_do_not_echo_the_active_credential() {
    let error = classify_backend_error(
        anyhow::anyhow!("proxy rejected current-secret"),
        "current-secret",
    );
    assert!(matches!(error, VoiceError::Transport { .. }));
    assert_eq!(error.to_string(), "proxy rejected [REDACTED]");
}

#[test]
fn backend_failure_preserves_the_context_chain_without_echoing_credentials() {
    let error = classify_backend_error(
        anyhow::anyhow!("connection refused using current-secret").context("voice request failed"),
        "current-secret",
    );
    assert!(matches!(error, VoiceError::Transport { .. }));
    assert_eq!(
        error.to_string(),
        "voice request failed: connection refused using [REDACTED]"
    );
}

#[test]
fn backend_failure_redaction_marker_does_not_echo_the_active_credential() {
    for detail in ["proxy rejected REDACTED", "REDREDACTEDACTED"] {
        let error = classify_backend_error(anyhow::anyhow!(detail), "REDACTED");
        assert!(matches!(error, VoiceError::Transport { .. }));
        assert!(
            !error.to_string().contains("REDACTED"),
            "redaction reintroduced the credential from {detail:?}"
        );
    }
}
