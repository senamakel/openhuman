use super::check_embedding_model_health;
use crate::config::Config;
use crate::doctor::core::Severity;

#[test]
fn embedding_probe_uses_configured_ollama_base_url() {
    let mut config = Config::default();
    config.memory.embedding_provider = "ollama".to_string();
    config.memory.embedding_model = "bge-m3".to_string();
    config.local_ai.base_url = Some("http://127.0.0.1:1".to_string());

    let mut items = Vec::new();
    check_embedding_model_health(&config, &mut items);

    let item = items
        .iter()
        .find(|item| item.category == "embedding_model")
        .expect("embedding check reports a result");
    assert_eq!(item.severity, Severity::Error);
    assert!(
        item.message.contains("http://127.0.0.1:1"),
        "probe should report the configured base URL, got: {}",
        item.message
    );
}

#[test]
fn non_ollama_embedding_provider_is_skipped() {
    let mut config = Config::default();
    config.memory.embedding_provider = "openai".to_string();

    let mut items = Vec::new();
    check_embedding_model_health(&config, &mut items);

    let item = items
        .iter()
        .find(|item| item.category == "embedding_model")
        .expect("embedding check reports a result");
    assert_eq!(item.severity, Severity::Skipped);
    assert!(
        item.message.contains("probed in the live tier"),
        "skip explains where the provider is probed, got: {}",
        item.message
    );
}
