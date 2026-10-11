//! The installed TinyHumans provider owns cloud voice HTTP and current credentials.

#[path = "support/tinyhumans_boot.rs"]
mod tinyhumans_boot;

use openhuman_core::config::Config;
use openhuman_core::security::credentials::{
    api_key, AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME,
};
use openhuman_core::voice::{
    cloud_transcribe::{transcribe_cloud, CloudTranscribeOptions},
    realtime::mint_voice_agent_signed_url,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn voice_config(root: &std::path::Path, endpoint: &str) -> Config {
    Config {
        workspace_dir: root.join("workspace"),
        action_dir: root.join("action"),
        config_path: root.join("config.toml"),
        api_url: Some(endpoint.into()),
        ..Config::default()
    }
}

#[tokio::test]
async fn cloud_calls_use_current_credentials_and_preserve_multipart_and_session_contracts() {
    tinyhumans_boot::boot();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"text":" recognized words "})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/voice-agent/get-signed-url"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"success":true,"data":{"signedUrl":"wss://voice.invalid/session","agentId":"agent","userToken":"user-binding"}})))
        .mount(&server).await;
    let root = tempfile::tempdir().unwrap();
    let config = voice_config(root.path(), &server.uri());
    api_key::store_api_key(&config, "first-api-key").unwrap();
    let options = CloudTranscribeOptions {
        language: Some("fr".into()),
        ..Default::default()
    };
    assert_eq!(
        transcribe_cloud(&config, "AQID", &options)
            .await
            .unwrap()
            .value
            .text,
        "recognized words"
    );
    api_key::store_api_key(&config, "rotated-api-key").unwrap();
    assert_eq!(
        transcribe_cloud(&config, "AQID", &options)
            .await
            .unwrap()
            .value
            .text,
        "recognized words"
    );
    let signed = mint_voice_agent_signed_url(&config).await.unwrap().value;
    assert_eq!(signed.signed_url, "wss://voice.invalid/session");
    assert_eq!(signed.user_token, "user-binding");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0].headers.get("authorization").unwrap(),
        "Bearer first-api-key"
    );
    assert_eq!(
        requests[1].headers.get("authorization").unwrap(),
        "Bearer rotated-api-key"
    );
    assert_eq!(
        requests[2].headers.get("x-api-key").unwrap(),
        "rotated-api-key"
    );
    assert!(requests[2].headers.get("authorization").is_none());
    assert!(requests[0]
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("multipart/form-data; boundary="));
    assert!(requests[0].body.windows(3).any(|bytes| bytes == [1, 2, 3]));
    let body = String::from_utf8_lossy(&requests[0].body);
    for expected in [
        "name=\"file\"",
        "name=\"model\"",
        "whisper-v1",
        "name=\"language\"",
        "fr",
    ] {
        assert!(body.contains(expected), "missing {expected}");
    }
}

#[tokio::test]
async fn cloud_authentication_errors_remain_classified_without_credential_echoes() {
    tinyhumans_boot::boot();
    let server = MockServer::start().await;
    let secret = "signed-in-session-secret";
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({"error":secret})))
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let config = voice_config(root.path(), &server.uri());
    AuthService::from_config(&config)
        .store_provider_token(
            APP_SESSION_PROVIDER,
            DEFAULT_AUTH_PROFILE_NAME,
            secret,
            std::collections::HashMap::new(),
            true,
        )
        .unwrap();
    let error = transcribe_cloud(&config, "AQID", &Default::default())
        .await
        .unwrap_err();
    assert!(error.starts_with("SESSION_EXPIRED:"), "{error}");
    assert!(!error.contains(secret));
    api_key::store_api_key(&config, "api-key-secret").unwrap();
    let error = transcribe_cloud(&config, "AQID", &Default::default())
        .await
        .unwrap_err();
    assert!(error.starts_with("API_KEY_REJECTED:"), "{error}");
    assert!(!openhuman_core::core::observability::is_session_expired_message(&error));
}

#[tokio::test]
async fn cloud_transcription_bounds_responses_and_refuses_unsafe_or_offline_credentials() {
    tinyhumans_boot::boot();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"text":"x".repeat(65_536)})),
        )
        .mount(&server)
        .await;
    let root = tempfile::tempdir().unwrap();
    let mut config = voice_config(root.path(), &server.uri());
    api_key::store_api_key(&config, "test-api-key").unwrap();
    assert!(transcribe_cloud(&config, "AQID", &Default::default())
        .await
        .unwrap_err()
        .contains("65536"));
    config.api_url = Some("http://backend.example.test".into());
    assert!(transcribe_cloud(&config, "AQID", &Default::default())
        .await
        .unwrap_err()
        .contains("refusing to send"));
    let offline = tempfile::tempdir().unwrap();
    let config = voice_config(offline.path(), &server.uri());
    AuthService::from_config(&config)
        .store_provider_token(
            APP_SESSION_PROVIDER,
            DEFAULT_AUTH_PROFILE_NAME,
            "desktop.test.local",
            std::collections::HashMap::new(),
            true,
        )
        .unwrap();
    let error = transcribe_cloud(&config, "AQID", &Default::default())
        .await
        .unwrap_err();
    assert!(
        openhuman_core::core::observability::is_backend_unavailable_message(&error),
        "{error}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
