use super::util::interrupted_pull_settle_window_secs;

use crate::config::Config;
use crate::inference::local::service::LocalAiService;
use axum::{routing::get, Json, Router};
use serde_json::json;

async fn spawn_mock(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{}", addr.port())
}

fn lm_studio_config(base: &str) -> Config {
    let mut config = Config::default();
    config.local_ai.runtime_enabled = true;
    config.local_ai.opt_in_confirmed = true;
    config.local_ai.provider = "lm_studio".to_string();
    config.local_ai.base_url = Some(format!("{base}/v1"));
    config.local_ai.model_id = "local-model".to_string();
    config.local_ai.chat_model_id = "local-model".to_string();
    config
}

#[path = "ollama_admin_discovery_fallback_tests.rs"]
mod discovery_fallback_tests;
#[path = "ollama_admin_health_and_diagnostics_tests.rs"]
mod health_and_diagnostics_tests;
#[path = "ollama_admin_lm_studio_and_lifecycle_tests.rs"]
mod lm_studio_and_lifecycle_tests;
