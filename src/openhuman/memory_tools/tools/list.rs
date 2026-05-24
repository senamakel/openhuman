//! `memory_tools_list` — list every stored rule for a given tool.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::openhuman::memory::ops::helpers::active_memory_client;
use crate::openhuman::memory_tools::ToolMemoryStore;
use crate::openhuman::tools::traits::{Tool, ToolResult};

pub struct MemoryToolsListTool;

#[derive(Debug, Deserialize)]
struct Args {
    tool_name: String,
}

#[async_trait]
impl Tool for MemoryToolsListTool {
    fn name(&self) -> &str {
        "memory_tools_list"
    }

    fn description(&self) -> &str {
        "List every stored memory rule for the given tool. Rules are durable \
         learnings about how to use the tool — priorities, gotchas, user \
         edicts. Returns the rules ordered by priority (Critical → Low) and \
         updated_at DESC within each priority."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["tool_name"],
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Exact tool name (e.g. `bash`, `web_search`)."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let parsed: Args = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("invalid arguments for memory_tools_list: {e}"))?;
        log::debug!("[tool][memory_tools] list tool_name={}", parsed.tool_name);
        let client = active_memory_client()
            .await
            .map_err(|e| anyhow::anyhow!("memory_tools_list: {e}"))?;
        let store = ToolMemoryStore::new(client.memory_handle());
        let rules = store
            .list_rules(&parsed.tool_name)
            .await
            .map_err(|e| anyhow::anyhow!("memory_tools_list: {e}"))?;
        let json = serde_json::to_string(&rules)?;
        Ok(ToolResult::success(json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::tools::traits::Tool;
    use serde_json::json;

    #[test]
    fn args_require_tool_name() {
        let args: Args = serde_json::from_value(json!({ "tool_name": "bash" })).unwrap();
        assert_eq!(args.tool_name, "bash");
    }

    #[test]
    fn parameters_schema_requires_tool_name() {
        let tool = MemoryToolsListTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["tool_name"]));
        assert_eq!(schema["properties"]["tool_name"]["type"], "string");
    }

    #[tokio::test]
    async fn execute_rejects_missing_tool_name() {
        let tool = MemoryToolsListTool;
        let err = tool
            .execute(json!({}))
            .await
            .expect_err("missing tool_name should fail");
        assert!(
            err.to_string()
                .contains("invalid arguments for memory_tools_list")
        );
    }

    #[tokio::test]
    async fn execute_success_path_returns_json_array() {
        let tool = MemoryToolsListTool;
        let result = tool
            .execute(json!({ "tool_name": "bash" }))
            .await
            .expect("valid tool list request should succeed");
        assert!(!result.is_error);
        let payload = result.text();
        let parsed: serde_json::Value =
            serde_json::from_str(&payload).expect("result should be valid json");
        assert!(
            parsed.is_array(),
            "list tool rules should serialize a JSON array"
        );
    }
}
