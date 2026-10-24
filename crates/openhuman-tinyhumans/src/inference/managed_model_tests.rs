use super::*;
use tinyinference_llm::message::Message;
use tinyinference_llm::model::{ChatModel, ModelRequest};

#[test]
fn managed_model_preserves_the_openhuman_response_cache_identity() {
    let model = build_chat_model(
        Some("https://api.example.test"),
        &ManagedOptions::default(),
        "reasoning-v1",
        true,
        None,
        false,
    )
    .expect("managed owner model");

    assert_eq!(
        model.cache_identity().as_deref(),
        Some("openhuman:https://api.example.test/openai/v1:reasoning-v1")
    );
}

#[test]
fn managed_factory_passes_product_default_to_native_model_when_configured_model_is_blank() {
    let configured = build_chat_model(
        Some("https://api.example.test"),
        &ManagedOptions::default(),
        "  reasoning-v1  ",
        true,
        None,
        false,
    )
    .expect("configured model");
    assert_eq!(
        configured.cache_identity().as_deref(),
        Some("openhuman:https://api.example.test/openai/v1:reasoning-v1")
    );

    let blank = build_chat_model(
        Some("https://api.example.test"),
        &ManagedOptions::default(),
        "  ",
        true,
        None,
        false,
    )
    .expect("blank model falls back to the product default");
    assert_eq!(
        blank.cache_identity().as_deref(),
        Some("openhuman:https://api.example.test/openai/v1:openrouter/deepseek/deepseek-v4-flash")
    );
}

#[test]
fn managed_native_model_exposes_tool_vision_and_physical_attempt_capabilities() {
    use tinyinference_llm::model::{InputModality, InputSource};

    let model = build_chat_model(
        Some("https://api.example.test"),
        &ManagedOptions::default(),
        "reasoning-v1",
        true,
        None,
        false,
    )
    .expect("managed native model");

    let profile = model.profile().expect("managed model profile");
    assert!(profile.tool_calling);
    assert!(profile.modalities.image_in);
    assert!(model.tracks_physical_attempts());
    assert!(model.supports_input(InputModality::Image, "image/png", InputSource::Base64));
}

#[test]
fn managed_bearer_keeps_the_stored_key_transport_guard() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    openhuman_embed::__host::security::credentials::api_key::store_api_key_in(
        tmp.path(),
        false,
        "test-managed-key",
    )
    .expect("store test API key");
    let options = ManagedOptions {
        openhuman_dir: Some(tmp.path().to_path_buf()),
        secrets_encrypt: false,
        ..ManagedOptions::default()
    };

    assert_eq!(
        options
            .resolve_bearer("https://api.tinyhumans.ai/openai/v1")
            .expect("managed TLS endpoint accepts the API key"),
        "test-managed-key"
    );
    assert_eq!(
        options
            .resolve_bearer("http://127.0.0.1:9999/openai/v1")
            .expect("loopback is a valid local test endpoint"),
        "test-managed-key"
    );
    let plaintext = options
        .resolve_bearer("http://api.example.test/openai/v1")
        .expect_err("a plaintext remote endpoint must not receive the stored key");
    assert!(plaintext.to_string().contains("http://api.example.test"));
    let error = options
        .resolve_bearer("https://api.example.test/openai/v1")
        .expect_err("a foreign HTTPS origin must not receive the stored key");
    assert!(error.to_string().contains("unmanaged or insecure"));
}

fn seed_app_session(dir: &std::path::Path, expires_at: Option<&str>) {
    use openhuman_embed::__host::security::credentials::{
        session_support::SESSION_EXPIRES_AT_META, AuthService, APP_SESSION_PROVIDER,
        DEFAULT_AUTH_PROFILE_NAME,
    };
    let mut metadata = std::collections::HashMap::new();
    if let Some(expires_at) = expires_at {
        metadata.insert(SESSION_EXPIRES_AT_META.to_string(), expires_at.to_string());
    }
    AuthService::new(dir, false)
        .store_provider_token(
            APP_SESSION_PROVIDER,
            DEFAULT_AUTH_PROFILE_NAME,
            "test.session.jwt",
            metadata,
            true,
        )
        .expect("seed app-session token");
}

fn managed_options_at(dir: &std::path::Path) -> ManagedOptions {
    ManagedOptions {
        openhuman_dir: Some(dir.to_path_buf()),
        secrets_encrypt: false,
        ..ManagedOptions::default()
    }
}

#[test]
fn managed_bearer_fast_fails_an_expired_app_session() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    seed_app_session(tmp.path(), Some(&past));

    let error = managed_options_at(tmp.path())
        .resolve_bearer("http://127.0.0.1:9/openai/v1")
        .expect_err("expired session should fail locally before transport");
    let message = error.to_string();
    assert!(message.contains("SESSION_EXPIRED"), "{message}");
    assert!(
        openhuman_embed::__host::core::observability::is_session_expired_message(&message),
        "expired managed session must retain its host classification: {message}"
    );
}

#[test]
fn managed_bearer_resolves_a_future_expiry_app_session() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    seed_app_session(tmp.path(), Some(&future));

    assert_eq!(
        managed_options_at(tmp.path())
            .resolve_bearer("http://127.0.0.1:9/openai/v1")
            .expect("a future expiry is valid"),
        "test.session.jwt"
    );
}

#[test]
fn managed_session_bearer_refuses_a_plaintext_remote_endpoint() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    seed_app_session(tmp.path(), Some("2999-01-01T00:00:00Z"));
    let options = managed_options_at(tmp.path());

    let error = options
        .resolve_bearer("http://10.0.0.1:1/openai/v1")
        .expect_err("plaintext remote endpoint must not receive the session token");
    assert!(error.to_string().contains("insecure endpoint"), "{error}");
    assert_eq!(
        options
            .resolve_bearer("https://api.example.test/openai/v1")
            .expect("HTTPS stays allowed"),
        "test.session.jwt"
    );
}

#[test]
fn managed_bearer_resolves_exp_less_hosted_app_sessions() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    seed_app_session(tmp.path(), None);

    assert_eq!(
        managed_options_at(tmp.path())
            .resolve_bearer("http://127.0.0.1:9/openai/v1")
            .expect("an exp-less hosted session resolves by presence"),
        "test.session.jwt"
    );
}

#[test]
fn managed_api_key_precedes_an_expired_app_session() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    seed_app_session(tmp.path(), Some(&past));
    openhuman_embed::__host::security::credentials::api_key::store_api_key_in(
        tmp.path(),
        false,
        "managed-api-key",
    )
    .expect("store API key");

    assert_eq!(
        managed_options_at(tmp.path())
            .resolve_bearer("https://api.tinyhumans.ai/openai/v1")
            .expect("the configured API key takes precedence"),
        "managed-api-key"
    );
}

#[test]
fn managed_bearer_refusal_redacts_foreign_endpoint_details() {
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    openhuman_embed::__host::security::credentials::api_key::store_api_key_in(
        tmp.path(),
        false,
        "SYNTHETIC_BEARER_MARKER",
    )
    .expect("store test API key");
    let options = ManagedOptions {
        openhuman_dir: Some(tmp.path().to_path_buf()),
        secrets_encrypt: false,
        ..ManagedOptions::default()
    };
    for (endpoint, markers) in [
        (
            "https://synthetic-user:SYNTHETIC_PASSWORD@foreign.example/private/SYNTHETIC_PATH?token=SYNTHETIC_QUERY#SYNTHETIC_FRAGMENT",
            [
                "synthetic-user",
                "SYNTHETIC_PASSWORD",
                "SYNTHETIC_PATH",
                "SYNTHETIC_QUERY",
                "SYNTHETIC_FRAGMENT",
            ],
        ),
        (
            "https://foreign.example/private/SYNTHETIC_PATH?token=SYNTHETIC_QUERY#SYNTHETIC_FRAGMENT",
            [
                "unused-userinfo",
                "unused-password",
                "SYNTHETIC_PATH",
                "SYNTHETIC_QUERY",
                "SYNTHETIC_FRAGMENT",
            ],
        ),
    ] {
        let error = options
            .resolve_bearer(endpoint)
            .expect_err("foreign endpoint must remain rejected");
        let diagnostic = error.to_string();
        for marker in markers.into_iter().chain(["SYNTHETIC_BEARER_MARKER"]) {
            assert!(
                !diagnostic.contains(marker),
                "refusal diagnostic exposed {marker}: {diagnostic}"
            );
        }
        assert!(diagnostic.contains("refusing to send"), "{diagnostic}");
    }
}

#[test]
fn managed_endpoint_diagnostic_is_origin_only_or_a_constant_fallback() {
    assert_eq!(
        endpoint_origin_for_log(
            "https://synthetic-user:SYNTHETIC_PASSWORD@foreign.example/private/SYNTHETIC_PATH?token=SYNTHETIC_QUERY#SYNTHETIC_FRAGMENT"
        ),
        "https://foreign.example"
    );
    assert_eq!(
        endpoint_origin_for_log("data:text/plain,SYNTHETIC_PATH"),
        "<invalid endpoint>"
    );
    assert_eq!(endpoint_origin_for_log("not a url"), "<invalid endpoint>");
}

async fn unauthorized_chat_response() -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": { "message": "HOST_AUTH_CALLBACK_MARKER" }
        })),
    )
        .into_response()
}

#[tokio::test]
async fn owner_typed_managed_auth_callback_publishes_session_expiry() {
    use openhuman_embed::__host::core::events::DomainEvent;

    openhuman_embed::__host::core::bus::init()
        .await
        .expect("initialize event bus");
    let mut events = openhuman_embed::__host::core::bus::BUS
        .get()
        .expect("event bus initialized")
        .receiver();
    let tmp = tempfile::TempDir::new().expect("temporary credentials");
    openhuman_embed::__host::security::credentials::api_key::store_api_key_in(
        tmp.path(),
        false,
        "test-managed-key",
    )
    .expect("store test API key");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let addr = listener.local_addr().expect("fixture address");
    let app = axum::Router::new().route(
        "/openai/v1/chat/completions",
        axum::routing::post(unauthorized_chat_response),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let options = ManagedOptions {
        openhuman_dir: Some(tmp.path().to_path_buf()),
        secrets_encrypt: false,
        ..ManagedOptions::default()
    };
    let model = build_chat_model(
        Some(&format!("http://{addr}")),
        &options,
        "reasoning-v1",
        true,
        None,
        true,
    )
    .expect("build owner model");

    assert!(model
        .invoke(&(), ModelRequest::new(vec![Message::user("ping")]))
        .await
        .is_err());
    server.abort();

    let mut source = None;
    loop {
        match events.try_recv() {
            Ok(DomainEvent::SessionExpired {
                source: value,
                reason,
            }) if reason.contains("HOST_AUTH_CALLBACK_MARKER") => {
                source = Some(value);
                break;
            }
            Ok(_) | Err(tinybus::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    assert_eq!(
        source.as_deref(),
        Some("openhuman_backend_model.invoke(401)"),
        "the owner's explicit managed-auth callback must preserve invoke attribution"
    );
}
