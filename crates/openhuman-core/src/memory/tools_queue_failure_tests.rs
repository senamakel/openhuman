use super::MemoryTool;
use crate::memory::test_fixtures::{bind_reference, config_in};
use tinytools::Tool;

#[tokio::test]
async fn write_queue_filesystem_failure_is_model_visible_through_execute() {
    let tmp = tempfile::tempdir().unwrap();
    let config = config_in(&tmp);
    bind_reference(&config);
    std::fs::write(config.workspace_dir.join("memory"), b"not a directory").unwrap();

    let result = MemoryTool::new(std::sync::Arc::new(config))
        .execute(serde_json::json!({
            "action": "learn",
            "text": "a durable memory fact"
        }))
        .await
        .unwrap();

    assert!(result.is_error, "{}", result.text());
    assert!(
        result.text().contains("memory write queue unavailable"),
        "{}",
        result.text()
    );
    assert!(result.text().contains("UNAVAILABLE"), "{}", result.text());
}
