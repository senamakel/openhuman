use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::memory::retrieval;
use crate::openhuman::memory::retrieval::rpc::DrillDownRequest;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct MemoryTreeDrillDownTool;

#[async_trait]
impl Tool for MemoryTreeDrillDownTool {
    fn name(&self) -> &str {
        "memory_tree_drill_down"
    }

    fn description(&self) -> &str {
        "Walk a summary node's children one step (or more if `max_depth > \
         1`). Returns leaf chunks for an L1 summary, or lower-level \
         summaries for L2+. Use this when a `query_*` summary is too coarse \
         and you want to expand it. Pass `query` to rerank children by \
         cosine similarity."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": {
                    "type": "string",
                    "description": "Id of the summary (or leaf) to expand."
                },
                "max_depth": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "How many levels down to walk (default 1)."
                },
                "query": {
                    "type": "string",
                    "description": "Optional natural-language query — when set, children are reranked by cosine similarity."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Optional cap on returned hits, applied after rerank."
                }
            },
            "required": ["node_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][memory_tree] drill_down invoked");
        let req: DrillDownRequest = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("invalid arguments for memory_tree_drill_down: {e}"))?;
        if matches!(req.max_depth, Some(0)) {
            return Err(anyhow::anyhow!(
                "memory_tree_drill_down: max_depth must be >= 1"
            ));
        }
        let cfg = config_rpc::load_config_with_timeout()
            .await
            .map_err(|e| anyhow::anyhow!("memory_tree_drill_down: load config failed: {e}"))?;
        let hits = retrieval::drill_down(
            &cfg,
            &req.node_id,
            req.max_depth.unwrap_or(1),
            req.query.as_deref(),
            req.limit,
        )
        .await?;
        log::debug!(
            "[tool][memory_tree] drill_down returning hits={}",
            hits.len()
        );
        let json = serde_json::to_string(&hits)?;
        Ok(ToolResult::success(json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parameters_schema_requires_node_id() {
        let tool = MemoryTreeDrillDownTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], json!(["node_id"]));
        assert_eq!(schema["properties"]["max_depth"]["minimum"], 1);
    }

    #[test]
    fn drill_down_request_deserializes_optional_fields() {
        let req: DrillDownRequest = serde_json::from_value(json!({
            "node_id": "summary-1",
            "max_depth": 2,
            "query": "deployment blockers",
            "limit": 7
        }))
        .unwrap();
        assert_eq!(req.node_id, "summary-1");
        assert_eq!(req.max_depth, Some(2));
        assert_eq!(req.query.as_deref(), Some("deployment blockers"));
        assert_eq!(req.limit, Some(7));
    }
}
