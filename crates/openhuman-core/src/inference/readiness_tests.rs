use super::*;
use crate::security::credentials::{AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME};
use tempfile::TempDir;

#[tokio::test]
async fn cached_probe_serves_definitive_negative_results() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let tmp = TempDir::new().unwrap();
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    };
    AuthService::new(tmp.path(), false)
        .store_provider_token(
            APP_SESSION_PROVIDER,
            DEFAULT_AUTH_PROFILE_NAME,
            "test.session.jwt",
            std::collections::HashMap::new(),
            true,
        )
        .expect("seed app-session token");
    let hit_count = std::sync::Arc::new(AtomicUsize::new(0));
    // The core owns no hosted provider; the managed backend's readiness probe
    // is whatever the host registered. Register one that rejects the way the
    // backend does, and count how often the core actually asks it.
    let _guard = crate::inference::inference_test_guard_async().await;
    let _exclusive = crate::inference::provider::factory::test_provider_override::exclusive();
    let counter = hit_count.clone();
    let managed = crate::inference::registry::chat_model(
        crate::inference::provider::factory::PROVIDER_OPENHUMAN,
    )
    .expect("the unit-test managed provider is available");
    crate::inference::registry::register_chat_model(
        crate::inference::provider::factory::PROVIDER_OPENHUMAN,
        crate::inference::registry::ChatModelEntry {
            build: managed.build,
            probe: Some(std::sync::Arc::new(move |_role, _model, _config| {
                counter.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Err("API key not configured for provider".to_string()) })
            })),
        },
    );

    let first = cached_probe(&DispatchCtx::ambient(), "summarization", &config).await;
    assert!(first
        .unwrap_err()
        .to_ascii_lowercase()
        .contains("api key not configured for provider"));
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    assert!(
        cached_probe(&DispatchCtx::ambient(), "summarization", &config)
            .await
            .is_err()
    );
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    crate::inference::registry::unregister_chat_model(
        crate::inference::provider::factory::PROVIDER_OPENHUMAN,
    );
}
