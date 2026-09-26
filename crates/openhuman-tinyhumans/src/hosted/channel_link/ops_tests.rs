use super::*;
use crate::hosted::test_support;
use openhuman_core::core::observability::is_session_expired_message;
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn normalize_channel_validates_and_lowercases() {
    assert_eq!(normalize_channel(" Telegram ").unwrap(), "telegram");
    assert_eq!(normalize_channel("discord").unwrap(), "discord");
    assert_eq!(normalize_channel("  ").unwrap_err(), "channel is required");
    assert_eq!(
        normalize_channel("Slack").unwrap_err(),
        "unsupported channel: slack"
    );
}

#[tokio::test]
async fn link_token_validation_runs_before_the_credential() {
    let tmp = TempDir::new().unwrap();
    let config = test_support::config(&tmp, "http://127.0.0.1:9");
    assert_eq!(
        auth_create_channel_link_token(&config, "matrix")
            .await
            .unwrap_err(),
        "unsupported channel: matrix"
    );
}

#[tokio::test]
async fn link_token_round_trip_and_401_classification() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/channels/telegram/link-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"success": true, "data": {"linkToken": "lt"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/channels/discord/link-token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "Invalid token"})))
        .expect(1)
        .mount(&server)
        .await;
    let tmp = TempDir::new().unwrap();
    let config = test_support::signed_in(&tmp, &server.uri());
    let out = auth_create_channel_link_token(&config, "TELEGRAM")
        .await
        .unwrap();
    assert_eq!(out.value, json!({"linkToken": "lt"}));
    let err = auth_create_channel_link_token(&config, "discord")
        .await
        .unwrap_err();
    assert!(is_session_expired_message(&err), "{err}");
}
