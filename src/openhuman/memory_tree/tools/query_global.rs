use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::memory::retrieval;
use crate::openhuman::memory::retrieval::rpc::QueryGlobalRequest;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct MemoryTreeQueryGlobalTool;

#[async_trait]
impl Tool for MemoryTreeQueryGlobalTool {
    fn name(&self) -> &str {
        "memory_tree_query_global"
    }

    fn description(&self) -> &str {
        "Return the cross-source global digest for the last `time_window_days`. \
         The 7-day digest is also pre-loaded into the session context at \
         start, so only call this for a different window (e.g. 30 days, \
         1 day) or to refresh after new ingest."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "time_window_days": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Lookback window in days (e.g. 7 for weekly recap)."
                }
            },
            "required": ["time_window_days"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][memory_tree] query_global invoked");
        let req: QueryGlobalRequest = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("invalid arguments for memory_tree_query_global: {e}"))?;
        let cfg = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| anyhow::anyhow!("memory_tree_query_global: load config failed: {e}"))?;
        let resp = retrieval::query_global(&cfg, req.time_window_days).await?;
        log::debug!(
            "[tool][memory_tree] query_global returning hits={} total={}",
            resp.hits.len(),
            resp.total
        );
        let json = serde_json::to_string(&resp)?;
        Ok(ToolResult::success(json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::tools::traits::Tool;
    use serde_json::json;

    #[test]
    fn parameters_schema_requires_time_window_days() {
        let tool = MemoryTreeQueryGlobalTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], json!(["time_window_days"]));
        assert_eq!(schema["properties"]["time_window_days"]["minimum"], 1);
    }

    #[tokio::test]
    async fn execute_rejects_missing_time_window_days() {
        let tool = MemoryTreeQueryGlobalTool;
        let err = tool
            .execute(json!({}))
            .await
            .expect_err("missing time_window_days should fail");
        assert!(
            err.to_string()
                .contains("invalid arguments for memory_tree_query_global")
        );
    }

    #[tokio::test]
    async fn execute_rejects_wrong_type_for_time_window_days() {
        let tool = MemoryTreeQueryGlobalTool;
        let err = tool
            .execute(json!({"time_window_days": "seven"}))
            .await
            .expect_err("wrong type should fail");
        assert!(
            err.to_string()
                .contains("invalid arguments for memory_tree_query_global")
        );
    }

    #[tokio::test]
    async fn execute_success_path_returns_json_payload() {
        let tool = MemoryTreeQueryGlobalTool;
        let result = tool
            .execute(json!({"time_window_days": 7}))
            .await
            .expect("valid query_global should succeed in local test environment");
        assert!(!result.is_error);
        let payload = result.text();
        let parsed: serde_json::Value =
            serde_json::from_str(&payload).expect("result should be valid json");
        assert!(
            parsed.get("hits").is_some(),
            "payload should include hits array"
        );
        assert!(
            parsed.get("total").is_some(),
            "payload should include total count"
        );
    }
}
