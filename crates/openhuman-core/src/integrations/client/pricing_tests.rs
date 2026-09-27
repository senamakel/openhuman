use super::*;
use crate::security::credentials::session_support::BackendCredential;

#[test]
fn local_offline_token_never_constructs_a_hosted_integration_client() {
    let config = crate::config::Config::default();
    let backend_url = "https://api.example.test".to_owned();
    assert!(build_client_with_credential(
        &config,
        backend_url.clone(),
        Some(BackendCredential::Session("header.payload.local".into())),
    )
    .is_none());
    assert!(build_client_with_credential(
        &config,
        backend_url,
        Some(BackendCredential::Session("header.payload.jwt".into())),
    )
    .is_some());
}

#[test]
fn api_key_constructs_a_client_that_authenticates_with_the_key() {
    let config = crate::config::Config::default();
    let client = build_client_with_credential(
        &config,
        "https://api.tinyhumans.ai".to_owned(),
        Some(BackendCredential::ApiKey(" tiny_test_abc ".into())),
    )
    .expect("an API key alone must build the integrations client");
    assert!(client.uses_api_key());
    assert_eq!(client.auth_token, "tiny_test_abc");
    let headers = client.auth_headers().unwrap();
    assert_eq!(headers.get("x-api-key").unwrap(), "tiny_test_abc");
    assert!(headers.get(reqwest::header::AUTHORIZATION).is_none());
}

#[test]
fn api_key_rejects_foreign_or_plaintext_integration_endpoints() {
    for endpoint in ["https://example.com", "http://example.com"] {
        let client = IntegrationClient::new_with_credential(
            endpoint.to_owned(),
            BackendCredential::ApiKey("tiny_test_abc".into()),
        );
        assert!(client.validate_credential_endpoint().is_err());
        assert!(client.auth_headers().is_err());
    }
}

#[test]
fn missing_or_blank_credential_builds_no_client() {
    let config = crate::config::Config::default();
    let url = "https://api.example.test".to_owned();
    assert!(build_client_with_credential(&config, url.clone(), None).is_none());
    assert!(build_client_with_credential(
        &config,
        url,
        Some(BackendCredential::ApiKey("   ".into()))
    )
    .is_none());
}
