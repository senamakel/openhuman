use super::*;
use crate::security::{AutonomyLevel, SecurityPolicy};

use serde_json::json;
use std::sync::Arc;
fn test_security() -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy::default())
}

/// Spawn a throwaway axum mock bound to an ephemeral port and return its base
/// URL. Mirrors `start_mock_backend` in `client_tests.rs` so both HTTP-level
/// direct-mode tests share one setup model.
async fn start_mock_backend(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}", addr.port())
}

#[path = "direct_tests_constructor_and_wire_types_tests.rs"]
mod constructor_and_wire_types_tests;
#[path = "direct_tests_sanitization_and_connected_accounts_tests.rs"]
mod sanitization_and_connected_accounts_tests;
