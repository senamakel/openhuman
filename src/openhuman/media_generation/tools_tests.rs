use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use super::{MediaGenerateImageTool, MediaGenerateVideoTool, MediaListModelsTool};
use crate::openhuman::integrations::IntegrationClient;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory};

fn dummy_client() -> Arc<IntegrationClient> {
    // No requests are made in these tests; the URL/token are placeholders.
    Arc::new(IntegrationClient::new(
        "http://127.0.0.1:0".to_string(),
        "test-token".to_string(),
    ))
}

#[test]
fn image_tool_schema_and_metadata() {
    let tool = MediaGenerateImageTool::new(dummy_client(), PathBuf::from("/tmp"));
    assert_eq!(tool.name(), "media_generate_image");
    assert_eq!(tool.permission_level(), PermissionLevel::Execute);
    assert_eq!(tool.category(), ToolCategory::Workflow);

    let schema = tool.parameters_schema();
    assert_eq!(schema["required"], json!(["prompt"]));
    let props = schema["properties"].as_object().unwrap();
    for key in ["prompt", "model", "size", "n", "input_images", "seed"] {
        assert!(props.contains_key(key), "missing image property {key}");
    }
}

#[test]
fn video_tool_schema_and_metadata() {
    let tool = MediaGenerateVideoTool::new(dummy_client(), PathBuf::from("/tmp"));
    assert_eq!(tool.name(), "media_generate_video");
    assert_eq!(tool.permission_level(), PermissionLevel::Execute);
    assert_eq!(tool.category(), ToolCategory::Workflow);

    let schema = tool.parameters_schema();
    assert_eq!(schema["required"], json!(["prompt"]));
    let props = schema["properties"].as_object().unwrap();
    for key in [
        "prompt",
        "model",
        "input_image",
        "duration_seconds",
        "aspect_ratio",
        "negative_prompt",
        "seed",
    ] {
        assert!(props.contains_key(key), "missing video property {key}");
    }
}

#[test]
fn list_models_tool_metadata() {
    let tool = MediaListModelsTool::new(dummy_client());
    assert_eq!(tool.name(), "media_list_models");
    assert_eq!(tool.category(), ToolCategory::Workflow);
    assert!(tool.parameters_schema()["properties"]
        .as_object()
        .unwrap()
        .contains_key("include_upstream"));
}

#[tokio::test]
async fn image_tool_rejects_empty_prompt_without_network() {
    let tool = MediaGenerateImageTool::new(dummy_client(), PathBuf::from("/tmp"));
    let result = tool.execute(json!({ "prompt": "   " })).await.unwrap();
    assert!(result.is_error);
}

#[tokio::test]
async fn video_tool_rejects_missing_prompt_without_network() {
    let tool = MediaGenerateVideoTool::new(dummy_client(), PathBuf::from("/tmp"));
    let result = tool.execute(json!({ "model": "x" })).await.unwrap();
    assert!(result.is_error);
}
