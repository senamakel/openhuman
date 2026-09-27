use super::*;
use crate::security::credentials::session_support::BackendCredential;
use serde_json::json;
use std::sync::Arc;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct NamedTransport(&'static str);

#[async_trait]
impl BackendTransport for NamedTransport {
    async fn send_json(&self, _req: BackendRequest<'_>) -> Result<Value, BackendTransportError> {
        Ok(json!({"from": self.0}))
    }

    async fn send_multipart(
        &self,
        _req: BackendRequest<'_>,
        _form: reqwest::multipart::Form,
    ) -> Result<Value, BackendTransportError> {
        Ok(Value::Null)
    }

    fn http_client(&self, _profile: TransportProfile) -> reqwest::Client {
        reqwest::Client::new()
    }

    // URL and attribution answers delegate to the plain transport: this
    // double is installed process-wide while other tests resolve base URLs.
    fn base_url(&self, configured: Option<&str>, purpose: BaseUrlPurpose) -> String {
        plain::PlainHttpTransport::new().base_url(configured, purpose)
    }

    fn product_identity(&self) -> String {
        plain::TEST_PRODUCT_IDENTITY.to_string()
    }

    fn attribution_headers(&self) -> reqwest::header::HeaderMap {
        plain::PlainHttpTransport::new().attribution_headers()
    }

    fn name(&self) -> &'static str {
        self.0
    }
}

/// The global slot is process state; serialise the tests that touch it.
fn global_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[test]
fn unwrap_envelope_returns_data_and_strips_success_flag() {
    assert_eq!(
        unwrap_envelope(json!({"success": true, "data": {"a": 1}})).unwrap(),
        json!({"a": 1})
    );
    assert_eq!(
        unwrap_envelope(json!({"success": true, "jwt": "t"})).unwrap(),
        json!({"jwt": "t"})
    );
    assert_eq!(
        unwrap_envelope(json!({"plain": true})).unwrap(),
        json!({"plain": true})
    );
    assert_eq!(unwrap_envelope(json!([1, 2])).unwrap(), json!([1, 2]));
}

#[test]
fn unwrap_envelope_turns_failure_into_typed_error() {
    let err = unwrap_envelope(json!({
        "success": false,
        "error": "Insufficient balance",
        "errorCode": "E_BAL",
        "details": {"needed": 3}
    }))
    .unwrap_err();
    match err {
        BackendTransportError::Envelope {
            error,
            error_code,
            details,
        } => {
            assert_eq!(error, "Insufficient balance");
            assert_eq!(error_code.as_deref(), Some("E_BAL"));
            assert_eq!(details, json!({"needed": 3}));
        }
        other => panic!("expected Envelope, got {other:?}"),
    }
    // `message` is accepted as the failure text, and a bare `success:false`
    // still fails with a generic reason rather than being handed back as data.
    assert!(matches!(
        unwrap_envelope(json!({"success": false, "message": "nope"})),
        Err(BackendTransportError::Envelope { error, .. }) if error == "nope"
    ));
    assert!(matches!(
        unwrap_envelope(json!({"success": false})),
        Err(BackendTransportError::Envelope { error, .. }) if error == "request unsuccessful"
    ));
}

#[test]
fn credential_headers_put_session_on_bearer_and_api_key_on_x_api_key() {
    let session =
        credential_headers(&BackendCredential::Session(" jwt.token.sig \n".into())).unwrap();
    assert_eq!(
        session.get(reqwest::header::AUTHORIZATION).unwrap(),
        "Bearer jwt.token.sig"
    );
    assert!(session.get(API_KEY_HEADER).is_none());

    let key = credential_headers(&BackendCredential::ApiKey("th_key".into())).unwrap();
    assert_eq!(key.get(API_KEY_HEADER).unwrap(), "th_key");
    assert!(key.get(reqwest::header::AUTHORIZATION).is_none());
}

#[test]
fn compose_url_joins_base_path_and_omits_none_query_values() {
    let url = compose_url(
        "https://api.example.test/",
        "teams/me/usage",
        &[("a", Some("1".into())), ("b", None)],
    )
    .unwrap();
    assert_eq!(url.as_str(), "https://api.example.test/teams/me/usage?a=1");
    let url = compose_url("https://api.example.test", "/x", &[]).unwrap();
    assert_eq!(url.as_str(), "https://api.example.test/x");
    assert!(matches!(
        compose_url("not a url", "/x", &[]),
        Err(BackendTransportError::Url(_))
    ));
}

#[test]
fn parse_body_text_handles_empty_json_and_plain_text() {
    assert_eq!(parse_body_text(String::new()), Value::Null);
    assert_eq!(parse_body_text("{\"a\":1}".into()), json!({"a": 1}));
    assert_eq!(parse_body_text("nope".into()), json!("nope"));
}

#[test]
fn global_install_replaces_and_clears() {
    let _guard = global_lock().lock().unwrap();
    clear_backend_transport();
    assert!(installed_backend_transport().is_none());

    install_backend_transport(Arc::new(NamedTransport("first")));
    assert_eq!(installed_backend_transport().unwrap().name(), "first");

    install_backend_transport(Arc::new(NamedTransport("second")));
    assert_eq!(installed_backend_transport().unwrap().name(), "second");

    clear_backend_transport();
    assert!(installed_backend_transport().is_none());
}

#[test]
fn resolve_prefers_global_over_test_fallback() {
    let _guard = global_lock().lock().unwrap();
    clear_backend_transport();
    // Under cfg(test) the plain transport is the floor, never `Unavailable`.
    assert_eq!(resolve_backend_transport().unwrap().name(), "plain-test");

    install_backend_transport(Arc::new(NamedTransport("host")));
    assert_eq!(resolve_backend_transport().unwrap().name(), "host");
    clear_backend_transport();
}

#[test]
fn error_status_accessor_only_reports_status_variant() {
    assert_eq!(
        BackendTransportError::Status {
            status: 404,
            body: Value::Null
        }
        .status(),
        Some(404)
    );
    assert_eq!(BackendTransportError::Unavailable.status(), None);
}

#[tokio::test]
async fn plain_transport_sends_attribution_and_credential_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/teams/me/usage"))
        .and(header("authorization", "Bearer jwt.a.b"))
        .and(header(
            plain::TEST_PRODUCT_HEADER,
            plain::TEST_PRODUCT_IDENTITY,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {"remainingUsd": 4.5}
        })))
        .mount(&server)
        .await;

    let transport = plain::PlainHttpTransport::new();
    let credential = BackendCredential::Session("jwt.a.b".into());
    let value = transport
        .send_json(BackendRequest {
            profile: TransportProfile::Api,
            base_url: &server.uri(),
            method: reqwest::Method::GET,
            path: "/teams/me/usage",
            query: &[],
            body: None,
            credential: Some(&credential),
            unwrap_envelope: true,
        })
        .await
        .unwrap();
    assert_eq!(value, json!({"remainingUsd": 4.5}));
}

#[tokio::test]
async fn plain_transport_maps_non_2xx_to_status_and_keeps_raw_when_not_unwrapping() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/agent-integrations/x"))
        .and(header(API_KEY_HEADER, "k1"))
        .respond_with(ResponseTemplate::new(402).set_body_string("Insufficient balance"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/raw"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true, "data": 1})))
        .mount(&server)
        .await;

    let transport = plain::PlainHttpTransport::new();
    let credential = BackendCredential::ApiKey("k1".into());
    let body = json!({"q": 1});
    let err = transport
        .send_json(BackendRequest {
            profile: TransportProfile::Integrations,
            base_url: &server.uri(),
            method: reqwest::Method::POST,
            path: "agent-integrations/x",
            query: &[],
            body: Some(&body),
            credential: Some(&credential),
            unwrap_envelope: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        BackendTransportError::Status { status: 402, ref body } if body == "Insufficient balance"
    ));

    let base = server.uri();
    let mut req = BackendRequest::new(TransportProfile::Api, &base, reqwest::Method::GET, "/raw");
    req.unwrap_envelope = false;
    assert_eq!(
        transport.send_json(req).await.unwrap(),
        json!({"success": true, "data": 1})
    );
}

#[tokio::test]
async fn backend_client_reports_unavailable_when_transport_returns_unavailable() {
    struct Absent;
    #[async_trait]
    impl BackendTransport for Absent {
        async fn send_json(
            &self,
            _req: BackendRequest<'_>,
        ) -> Result<Value, BackendTransportError> {
            Err(BackendTransportError::Unavailable)
        }
        async fn send_multipart(
            &self,
            _req: BackendRequest<'_>,
            _form: reqwest::multipart::Form,
        ) -> Result<Value, BackendTransportError> {
            Err(BackendTransportError::Unavailable)
        }
        fn http_client(&self, _profile: TransportProfile) -> reqwest::Client {
            reqwest::Client::new()
        }
        fn base_url(&self, configured: Option<&str>, purpose: BaseUrlPurpose) -> String {
            plain::PlainHttpTransport::new().base_url(configured, purpose)
        }
        fn product_identity(&self) -> String {
            plain::TEST_PRODUCT_IDENTITY.to_string()
        }
        fn attribution_headers(&self) -> reqwest::header::HeaderMap {
            plain::PlainHttpTransport::new().attribution_headers()
        }
        fn name(&self) -> &'static str {
            "absent"
        }
    }

    let _guard = global_lock().lock().unwrap();
    install_backend_transport(Arc::new(Absent));
    let client = crate::backend::BackendClient::new("https://api.example.test").unwrap();
    let err = client
        .authed_json("jwt", reqwest::Method::GET, "/payments/summary", None)
        .await
        .unwrap_err();
    clear_backend_transport();

    assert!(matches!(
        err.downcast_ref::<crate::backend::BackendApiError>(),
        Some(crate::backend::BackendApiError::BackendUnavailable { method, path })
            if method == "GET" && path == "/payments/summary"
    ));
    let flat = crate::backend::flatten_authed_error(err);
    assert!(flat.starts_with(crate::core::observability::BACKEND_UNAVAILABLE_PREFIX));
    assert_eq!(
        crate::core::observability::expected_error_kind(&flat),
        Some(crate::core::observability::ExpectedErrorKind::BackendUnavailable)
    );
}
