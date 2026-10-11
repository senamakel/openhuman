use super::*;

#[test]
fn backend_authentication_categories_survive_provider_dispatch() {
    for (error, expected) in [
        (
            BackendApiError::Unauthorized {
                method: "POST".into(),
                path: "/openai/v1/audio/speech".into(),
            },
            ProviderErrorKind::SessionExpired,
        ),
        (
            BackendApiError::ApiKeyRejected {
                method: "GET".into(),
                path: "/voice-agent/get-signed-url".into(),
            },
            ProviderErrorKind::ApiKeyRejected,
        ),
        (
            BackendApiError::BackendUnavailable {
                method: "POST".into(),
                path: "/agent-integrations/gemini/live/sessions".into(),
            },
            ProviderErrorKind::Unavailable,
        ),
    ] {
        assert_eq!(
            classify_backend_error(anyhow::Error::new(error), "").kind,
            expected
        );
    }
    let error = classify_backend_error(anyhow::anyhow!("connection refused"), "");
    assert_eq!(error.kind, ProviderErrorKind::Transport);
    assert_eq!(error.message, "connection refused");
}

#[test]
fn backend_failure_details_do_not_echo_the_active_credential() {
    let error = classify_backend_error(
        anyhow::anyhow!("proxy rejected current-secret"),
        "current-secret",
    );
    assert_eq!(error.kind, ProviderErrorKind::Transport);
    assert_eq!(error.message, "proxy rejected [REDACTED]");
}
