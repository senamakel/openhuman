use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn backend_401_is_tagged_as_session_expiry() {
    let tagged = classify_transcribe_error(
        "transcription request failed (401 Unauthorized): {\"error\":\"Invalid token\"}"
            .to_string(),
        false,
    );
    assert!(tagged.starts_with("SESSION_EXPIRED: "));
    assert!(crate::core::observability::is_session_expired_message(
        &tagged
    ));
}

#[test]
fn other_failures_pass_through_unchanged() {
    let error = "transcription request failed (500 Internal Server Error): boom".to_string();
    assert_eq!(classify_transcribe_error(error.clone(), false), error);
}

#[test]
fn backend_401_with_api_key_does_not_expire_session() {
    let tagged = classify_transcribe_error(
        "transcription request failed (401 Unauthorized)".to_string(),
        true,
    );
    assert!(tagged.starts_with("API_KEY_REJECTED: "));
    assert!(!crate::core::observability::is_session_expired_message(
        &tagged
    ));
}

#[tokio::test]
async fn offline_local_session_refuses_without_a_request() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    };
    crate::security::credentials::AuthService::from_config(&config)
        .store_provider_token(
            crate::security::credentials::APP_SESSION_PROVIDER,
            crate::security::credentials::DEFAULT_AUTH_PROFILE_NAME,
            "desktop.test.local",
            std::collections::HashMap::new(),
            true,
        )
        .unwrap();
    let err = transcribe_cloud(&config, "AAAA", &CloudTranscribeOptions::default())
        .await
        .unwrap_err();
    assert!(
        crate::core::observability::is_backend_unavailable_message(&err),
        "{err}"
    );
}

#[tokio::test]
async fn api_key_refuses_remote_plaintext_endpoint_before_request() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        api_url: Some("http://backend.example.test".to_string()),
        ..Config::default()
    };
    crate::security::credentials::api_key::store_api_key(&config, "test-api-key").unwrap();

    let err = transcribe_cloud(&config, "AAAA", &CloudTranscribeOptions::default())
        .await
        .unwrap_err();
    assert!(err.contains("refusing to send"), "{err}");
}

#[tokio::test]
async fn api_key_transcribes_via_safe_backend_with_bearer_and_multipart_audio() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "recognized words"
        })))
        .mount(&server)
        .await;

    let tmp = tempfile::TempDir::new().unwrap();
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        api_url: Some(server.uri()),
        ..Config::default()
    };
    crate::security::credentials::api_key::store_api_key(&config, "test-api-key").unwrap();

    let outcome = transcribe_cloud(&config, "AQID", &CloudTranscribeOptions::default())
        .await
        .unwrap()
        .value;
    assert_eq!(outcome.text, "recognized words");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.headers.get("authorization").unwrap(),
        "Bearer test-api-key"
    );
    assert!(request
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("multipart/form-data; boundary="));
    assert!(request.body.windows(3).any(|bytes| bytes == [1, 2, 3]));
    let body = String::from_utf8_lossy(&request.body);
    assert!(body.contains("name=\"file\""));
    assert!(body.contains("name=\"model\""));
}
