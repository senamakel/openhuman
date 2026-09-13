//! Dataset: `ParallelDatasetTool` (FindAll, async).

use crate::integrations::IntegrationClient;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ── ParallelDatasetTool ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DatasetResponse {
    #[serde(rename = "findallId", default)]
    findall_id: String,
    #[serde(default)]
    status: serde_json::Value,
    #[serde(rename = "matchLimit", default)]
    match_limit: u64,
    #[serde(rename = "costUsd", default)]
    cost_usd: f64,
}

/// Generate a web dataset via Parallel's FindAll — kicks off an async run
/// that produces structured candidate matches.
pub struct ParallelDatasetTool {
    client: Arc<IntegrationClient>,
}

impl ParallelDatasetTool {
    pub fn new(client: Arc<IntegrationClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ParallelDatasetTool {
    fn name(&self) -> &str {
        "parallel_dataset"
    }

    fn description(&self) -> &str {
        "Generate a web dataset via Parallel FindAll. Describe an `objective`, \
         the `entity_type` you want (e.g. \"SaaS company\", \"academic paper\"), \
         and a list of `match_conditions` — each a `name` plus an optional \
         `description`. Parallel discovers and enriches matching candidates \
         in the background. This call returns the run ID and pre-authorised cost; \
         use `match_limit` to cap how many candidates are produced. \
         Run is async — fetch results separately by `findall_id`."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "objective": { "type": "string", "description": "What dataset to build" },
                "entity_type": { "type": "string", "description": "What kind of entity to find" },
                "match_conditions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 20,
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "required": ["name"]
                    }
                },
                "generator": {
                    "type": "string",
                    "enum": ["preview", "base", "core", "pro"],
                    "description": "Generator tier (default base)"
                },
                "match_limit": {
                    "type": "integer",
                    "minimum": 5,
                    "maximum": 1000,
                    "description": "Max candidates to produce (default 10)"
                }
            },
            "required": ["objective", "entity_type", "match_conditions"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let objective = args
            .get("objective")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: objective"))?;
        let entity_type = args
            .get("entity_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: entity_type"))?;
        let match_conditions = args
            .get("match_conditions")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| anyhow::anyhow!("match_conditions must be a non-empty array"))?;

        let mut body = json!({
            "objective": objective,
            "entityType": entity_type,
            "matchConditions": match_conditions,
        });
        if let Some(g) = args.get("generator").and_then(|v| v.as_str()) {
            body["generator"] = json!(g);
        }
        if let Some(l) = args.get("match_limit").and_then(|v| v.as_u64()) {
            body["matchLimit"] = json!(l.clamp(5, 1000));
        }

        tracing::info!("[parallel_dataset] entity_type={}", entity_type);

        match self
            .client
            .post::<DatasetResponse>("/agent-integrations/parallel/dataset", &body)
            .await
        {
            Ok(resp) => {
                let out = format!(
                    "Dataset run started\n  findall_id: {}\n  match_limit: {}\n  status: {}\n\nCost (pre-authorised): ${:.4}\n\nResults are produced asynchronously — fetch them later by findall_id.",
                    resp.findall_id,
                    resp.match_limit,
                    serde_json::to_string(&resp.status).unwrap_or_default(),
                    resp.cost_usd
                );
                Ok(ToolResult::success(out))
            }
            Err(e) => Ok(ToolResult::error(format!("Parallel dataset failed: {e}"))),
        }
    }
}
