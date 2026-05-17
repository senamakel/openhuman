//! Integration tests for the OpenAI-compatible `/v1` HTTP endpoint.
//!
//! These tests spin up an in-process axum router (no network), send
//! crafted HTTP requests via `tower::ServiceExt::oneshot`, and assert on
//! the response status codes.
//!
//! A running inference backend is NOT required — the tests exercise the
//! routing and auth-middleware layers only.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use crate::core::jsonrpc::build_core_http_router;

/// Build the test router (Socket.IO disabled — no real runtime needed).
fn test_router() -> axum::Router {
    build_core_http_router(false)
}

/// Convenience: dispatch a single request through the in-process router.
async fn dispatch(req: Request<Body>) -> axum::response::Response {
    test_router().oneshot(req).await.unwrap()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Requests to `POST /v1/chat/completions` without any `Authorization` header
/// must be rejected with `401 Unauthorized`.
#[tokio::test]
async fn test_chat_completions_no_bearer_returns_401() {
    let body = serde_json::json!({
        "model": "ollama:llama3",
        "messages": [{ "role": "user", "content": "hello" }]
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = dispatch(req).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "POST /v1/chat/completions without bearer must return 401"
    );
}

/// Requests to `GET /v1/models` without any `Authorization` header must be
/// rejected with `401 Unauthorized`.
#[tokio::test]
async fn test_models_no_bearer_returns_401() {
    let req = Request::builder()
        .method(Method::GET)
        .uri("/v1/models")
        .body(Body::empty())
        .unwrap();

    let resp = dispatch(req).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET /v1/models without bearer must return 401"
    );
}

/// A request with a bearer token must not be rejected as 401/403. The actual
/// response code depends on whether a live inference backend is running; the
/// test only asserts that auth passed.
#[tokio::test]
async fn test_chat_completions_with_bearer_not_rejected_as_auth_error() {
    // Use the env var if set (CI with a real core token), otherwise use any
    // non-empty string — the test-support middleware accepts it.
    let token = std::env::var("OPENHUMAN_CORE_TOKEN").unwrap_or_else(|_| "test-token".to_string());

    let body = serde_json::json!({
        "model": "ollama:llama3",
        "messages": [{ "role": "user", "content": "ping" }],
        "stream": false
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", token))
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = dispatch(req).await;
    let status = resp.status();
    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "401 must not fire when bearer is present"
    );
    assert_ne!(
        status,
        StatusCode::FORBIDDEN,
        "403 must not fire when bearer is present"
    );
}
