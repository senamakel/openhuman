//! Raw-line oriented E2E coverage for the Ollama embedding provider.
//!
//! These tests use the public embedding provider API against a local mock
//! Ollama HTTP server. They avoid a real daemon while exercising the same
//! request, validation, and NaN-recovery branches used in production.

use std::net::SocketAddr;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};

use openhuman_core::openhuman::embeddings::catalog;
use openhuman_core::openhuman::embeddings::cloud::{
    OpenHumanCloudEmbedding, DEFAULT_CLOUD_EMBEDDING_DIMENSIONS, DEFAULT_CLOUD_EMBEDDING_MODEL,
};
use openhuman_core::openhuman::embeddings::noop::NoopEmbedding;
use openhuman_core::openhuman::embeddings::ollama::DEFAULT_OLLAMA_URL;
use openhuman_core::openhuman::embeddings::retry_after::{
    backoff_ms_for_attempt, parse_retry_after_ms, BASE_BACKOFF_MS, MAX_BACKOFF_MS,
};
use openhuman_core::openhuman::embeddings::{
    create_embedding_provider, EmbeddingProvider, OllamaEmbedding, DEFAULT_OLLAMA_DIMENSIONS,
    DEFAULT_OLLAMA_MODEL,
};

async fn serve_mock_ollama(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock ollama");
    let addr: SocketAddr = listener.local_addr().expect("mock ollama addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock ollama serve");
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[test]
fn ollama_constructor_normalizes_defaults_and_rejects_runtime_misconfiguration() {
    let defaults = OllamaEmbedding::try_new("  ", "  ", 0).expect("default ollama config");
    assert_eq!(defaults.base_url(), DEFAULT_OLLAMA_URL);
    assert_eq!(defaults.model(), DEFAULT_OLLAMA_MODEL);
    assert_eq!(defaults.dimensions(), DEFAULT_OLLAMA_DIMENSIONS);

    let custom = OllamaEmbedding::try_new("http://[::1]:11434/", "  nomic-embed-text  ", 12)
        .expect("custom ollama config");
    assert_eq!(custom.base_url(), "http://[::1]:11434");
    assert_eq!(custom.model(), "nomic-embed-text");
    assert_eq!(
        custom.signature(),
        "provider=ollama;model=nomic-embed-text;dims=12"
    );

    for bad_url in [
        "ftp://localhost:11434",
        "http://user:pass@localhost:11434",
        "http://localhost:11434/api",
        "http://localhost:11434/v1/chat/completions",
        "http://localhost:11434?debug=true",
        "http://localhost:11434/#fragment",
    ] {
        assert!(
            OllamaEmbedding::try_new(bad_url, "m", 1).is_err(),
            "bad Ollama URL should be rejected: {bad_url}"
        );
    }
    assert!(OllamaEmbedding::try_new("http://localhost:11434", "local-v1", 1).is_err());
}

#[tokio::test]
async fn embedding_catalog_factory_retry_noop_and_cloud_empty_paths_are_reachable() {
    let providers = catalog::all_providers();
    assert!(providers.iter().any(|provider| provider.slug == "managed"));
    assert!(providers.iter().any(|provider| provider.slug == "cohere"));

    assert_eq!(parse_retry_after_ms(Some(" 5 ")), Some(5_000));
    assert_eq!(
        parse_retry_after_ms(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
        None
    );
    assert_eq!(parse_retry_after_ms(Some("99999")), Some(MAX_BACKOFF_MS));
    assert_eq!(backoff_ms_for_attempt(2, Some("1")), 1_000);
    assert_eq!(backoff_ms_for_attempt(1, None), BASE_BACKOFF_MS * 2);
    assert_eq!(backoff_ms_for_attempt(10, Some("bad")), MAX_BACKOFF_MS);

    let noop = NoopEmbedding;
    assert_eq!(noop.name(), "none");
    assert_eq!(
        noop.embed(&["ignored"]).await.expect("noop embed"),
        Vec::<Vec<f32>>::new()
    );

    let cloud = OpenHumanCloudEmbedding::new(
        Some("https://api.example.test/".to_string()),
        None,
        false,
        DEFAULT_CLOUD_EMBEDDING_MODEL,
        DEFAULT_CLOUD_EMBEDDING_DIMENSIONS,
    );
    assert_eq!(cloud.name(), "cloud");
    assert!(cloud
        .embed(&[])
        .await
        .expect("cloud empty embed")
        .is_empty());

    for (provider, model, dims, expected_name) in [
        (
            "managed",
            DEFAULT_CLOUD_EMBEDDING_MODEL,
            DEFAULT_CLOUD_EMBEDDING_DIMENSIONS,
            "cloud",
        ),
        ("voyage", "", 0, "voyage"),
        ("cohere", "", 0, "cohere"),
        ("openai", "text-embedding-3-small", 1536, "openai"),
        ("custom:http://127.0.0.1:9", "custom-embedding", 2, "openai"),
        ("none", "", 0, "none"),
    ] {
        let embedder =
            create_embedding_provider(provider, model, dims).expect("provider should construct");
        assert_eq!(embedder.name(), expected_name);
    }

    match create_embedding_provider("unknown", "m", 1) {
        Ok(_) => panic!("unknown provider should fail"),
        Err(err) => assert!(err.to_string().contains("unknown embedding provider")),
    }
}

#[tokio::test]
async fn ollama_embed_preserves_positions_and_validates_request_and_response() {
    let app = Router::new().route(
        "/api/embed",
        post(|Json(body): Json<Value>| async move {
            assert_eq!(
                body.get("model").and_then(Value::as_str),
                Some("mock-ollama")
            );
            assert_eq!(
                body.pointer("/input/0").and_then(Value::as_str),
                Some("alpha")
            );
            assert_eq!(
                body.pointer("/input/1").and_then(Value::as_str),
                Some("beta")
            );
            Json(json!({ "embeddings": [[1.0, 2.0], [3.0, 4.0]] }))
        }),
    );
    let base_url = serve_mock_ollama(app).await;
    let provider = OllamaEmbedding::try_new(&base_url, "mock-ollama", 2).expect("provider");

    let vectors = provider
        .embed(&[" alpha ", "", "beta", "   "])
        .await
        .expect("ollama embed");
    assert_eq!(
        vectors,
        vec![vec![1.0, 2.0], vec![], vec![3.0, 4.0], vec![]]
    );

    let all_blank = provider.embed(&["", " \n\t "]).await.expect("blank embed");
    assert_eq!(all_blank, vec![Vec::<f32>::new(), Vec::<f32>::new()]);
}

#[tokio::test]
async fn ollama_embed_reports_malformed_count_dimension_and_transport_errors() {
    let count_url = serve_mock_ollama(Router::new().route(
        "/api/embed",
        post(|| async { Json(json!({ "embeddings": [[1.0]] })) }),
    ))
    .await;
    let count_provider = OllamaEmbedding::try_new(&count_url, "m", 1).expect("count provider");
    assert!(count_provider
        .embed(&["a", "b"])
        .await
        .expect_err("count mismatch")
        .to_string()
        .contains("count mismatch"));

    let dim_url = serve_mock_ollama(Router::new().route(
        "/api/embed",
        post(|| async { Json(json!({ "embeddings": [[1.0, 2.0, 3.0]] })) }),
    ))
    .await;
    let dim_provider = OllamaEmbedding::try_new(&dim_url, "m", 2).expect("dim provider");
    assert!(dim_provider
        .embed(&["a"])
        .await
        .expect_err("dimension mismatch")
        .to_string()
        .contains("dimension mismatch"));

    let malformed_url = serve_mock_ollama(Router::new().route(
        "/api/embed",
        post(|| async { (StatusCode::OK, "not json") }),
    ))
    .await;
    let malformed_provider =
        OllamaEmbedding::try_new(&malformed_url, "m", 2).expect("malformed provider");
    assert!(malformed_provider
        .embed(&["a"])
        .await
        .expect_err("malformed response")
        .to_string()
        .contains("parse failed"));

    let refused = OllamaEmbedding::try_new("http://127.0.0.1:1", "m", 2).expect("refused provider");
    assert!(refused
        .embed(&["a"])
        .await
        .expect_err("connection refused")
        .to_string()
        .contains("is Ollama running"));
}

#[tokio::test]
async fn ollama_embed_recovers_nan_batch_with_per_text_fallback() {
    let app = Router::new().route(
        "/api/embed",
        post(|Json(body): Json<Value>| async move {
            let inputs = body
                .get("input")
                .and_then(Value::as_array)
                .expect("input array");
            if inputs.len() > 1 {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    r#"{"error":"failed to encode response: json: unsupported value: NaN"}"#
                        .to_string(),
                );
            }
            if inputs.first().and_then(Value::as_str) == Some("bad") {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unsupported value: nan".to_string(),
                );
            }
            (
                StatusCode::OK,
                json!({ "embeddings": [[9.0, 8.0]] }).to_string(),
            )
        }),
    );
    let base_url = serve_mock_ollama(app).await;
    let provider = OllamaEmbedding::try_new(&base_url, "mock-ollama", 2).expect("provider");

    let vectors = provider
        .embed(&["good", "bad", " "])
        .await
        .expect("nan batch recovery");
    assert_eq!(vectors, vec![vec![9.0, 8.0], vec![], vec![]]);

    let single_nan = provider.embed(&["bad"]).await.expect("single nan recovery");
    assert_eq!(single_nan, vec![Vec::<f32>::new()]);
}
