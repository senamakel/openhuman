//! Research: `ParallelResearchTool` and its response types.

use crate::integrations::IntegrationClient;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ── ParallelResearchTool ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct ResearchResponse {
    #[serde(default, rename = "runId")]
    pub(super) run_id: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) result: Option<serde_json::Value>,
    #[serde(rename = "costUsd", default)]
    pub(super) cost_usd: f64,
}

pub(super) fn format_research_response(resp: ResearchResponse) -> Result<String, String> {
    if let Some(id) = &resp.run_id {
        tracing::debug!(
            "[parallel_research] completed run_id={} status={:?} cost_usd={:.4}",
            id,
            resp.status,
            resp.cost_usd
        );
    } else {
        tracing::debug!(
            "[parallel_research] completed without run_id status={:?} cost_usd={:.4}",
            resp.status,
            resp.cost_usd
        );
    }

    let mut out = String::new();
    if let Some(s) = &resp.status {
        out.push_str(&format!("Status: {}\n", s));
    }
    let Some(r) = resp.result else {
        let status = resp.status.as_deref().unwrap_or("unknown");
        tracing::debug!(
            "[parallel_research] incomplete blocking response status={} cost_usd={:.4}",
            status,
            resp.cost_usd
        );
        return Err(format!(
            "Parallel research did not return a result before the inline wait completed (status: {status}). Try again with a higher timeout_seconds or a cheaper processor."
        ));
    };
    out.push_str("\nResult:\n");
    out.push_str(&serde_json::to_string_pretty(&r).unwrap_or_default());
    out.push_str(&format!("\n\nCost: ${:.4}", resp.cost_usd));
    Ok(out)
}

pub(super) fn research_payload(resp: &ResearchResponse, display: &str) -> serde_json::Value {
    json!({
        "display": display,
        "status": resp.status,
        "result": resp.result,
        "cost_usd": resp.cost_usd,
    })
}

/// Deep research via Parallel's Task API — multi-step web investigation
/// with structured or freeform output.
pub struct ParallelResearchTool {
    client: Arc<IntegrationClient>,
}

impl ParallelResearchTool {
    pub fn new(client: Arc<IntegrationClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ParallelResearchTool {
    fn name(&self) -> &str {
        "parallel_research"
    }

    fn description(&self) -> &str {
        "Deep web research via Parallel's Task API. Submit an objective and a processor \
         tier (`lite`, `base`, `core`, `ultra`) — Parallel browses many sources, \
         synthesises, and returns a single rich answer. Optionally pass an \
         `output_schema` (JSON schema) to force structured output. \
         Blocks inline until the run completes (up to ~10 minutes). \
         Use for tasks that need more than a single search/extract pair, e.g. \
         \"compare these three companies' financials\" or \"build a competitor matrix\"."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "input": {
                    "description": "The research objective — string or structured object",
                    "oneOf": [{ "type": "string" }, { "type": "object" }]
                },
                "processor": {
                    "type": "string",
                    "enum": ["lite", "base", "core", "ultra"],
                    "description": "Processor tier — lite (cheapest) → ultra (most thorough)"
                },
                "output_schema": {
                    "type": "object",
                    "description": "Optional JSON schema describing the desired structured output"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": 10,
                    "maximum": 900,
                    "description": "Max time to wait inline (default 600)"
                }
            },
            "required": ["input", "processor"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let input = args
            .get("input")
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: input"))?;
        let processor = args
            .get("processor")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: processor"))?;

        let mut body = json!({
            "input": input,
            "processor": processor,
            "wait": true,
        });
        if let Some(schema) = args.get("output_schema") {
            body["outputSchema"] = schema.clone();
        }
        if let Some(t) = args.get("timeout_seconds").and_then(|v| v.as_u64()) {
            body["timeoutSeconds"] = json!(t.clamp(10, 900));
        }

        tracing::info!("[parallel_research] processor={}", processor);

        match self
            .client
            .post::<ResearchResponse>("/agent-integrations/parallel/research", &body)
            .await
        {
            Ok(resp) => {
                let display = match format_research_response(ResearchResponse {
                    run_id: resp.run_id.clone(),
                    status: resp.status.clone(),
                    result: resp.result.clone(),
                    cost_usd: resp.cost_usd,
                }) {
                    Ok(display) => display,
                    Err(message) => return Ok(ToolResult::error(message)),
                };
                Ok(ToolResult::success_with_markdown(
                    research_payload(&resp, &display),
                    display,
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Parallel research failed: {e}"))),
        }
    }
}
