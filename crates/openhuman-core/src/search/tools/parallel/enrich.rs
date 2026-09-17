//! Enrich: `ParallelEnrichTool` and its response types.

use crate::integrations::IntegrationClient;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ── ParallelEnrichTool ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct EnrichResponse {
    #[serde(default, rename = "runId")]
    pub(super) run_id: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) output: Option<serde_json::Value>,
    #[serde(rename = "costUsd", default)]
    pub(super) cost_usd: f64,
}

pub(super) fn format_enrich_response(resp: EnrichResponse) -> Result<String, String> {
    if let Some(id) = &resp.run_id {
        tracing::debug!(
            "[parallel_enrich] completed run_id={} status={:?} cost_usd={:.4}",
            id,
            resp.status,
            resp.cost_usd
        );
    } else {
        tracing::debug!(
            "[parallel_enrich] completed without run_id status={:?} cost_usd={:.4}",
            resp.status,
            resp.cost_usd
        );
    }

    let mut out = String::new();
    if let Some(s) = &resp.status {
        out.push_str(&format!("Status: {}\n", s));
    }
    let Some(o) = resp.output else {
        let status = resp.status.as_deref().unwrap_or("unknown");
        tracing::debug!(
            "[parallel_enrich] incomplete blocking response status={} cost_usd={:.4}",
            status,
            resp.cost_usd
        );
        return Err(format!(
            "Parallel enrich did not return output before the inline wait completed (status: {status}). Try again with a higher timeout_seconds or a cheaper processor."
        ));
    };
    out.push_str("\nOutput:\n");
    out.push_str(&serde_json::to_string_pretty(&o).unwrap_or_default());
    out.push_str(&format!("\n\nCost: ${:.4}", resp.cost_usd));
    Ok(out)
}

pub(super) fn enrich_payload(resp: &EnrichResponse, display: &str) -> serde_json::Value {
    json!({
        "display": display,
        "status": resp.status,
        "output": resp.output,
        "cost_usd": resp.cost_usd,
    })
}

/// Enrich an entity with structured web data — synchronous Task API run
/// with a required output schema.
pub struct ParallelEnrichTool {
    client: Arc<IntegrationClient>,
}

impl ParallelEnrichTool {
    pub fn new(client: Arc<IntegrationClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ParallelEnrichTool {
    fn name(&self) -> &str {
        "parallel_enrich"
    }

    fn description(&self) -> &str {
        "Enrich an entity (company, person, product) with structured web data. \
         Pass an `input` (the thing to enrich) plus a JSON `output_schema` describing \
         the fields you want filled in — Parallel returns a structured object \
         conforming to that schema. Blocks until the run completes."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "input": {
                    "description": "Entity to enrich — string or object",
                    "oneOf": [{ "type": "string" }, { "type": "object" }]
                },
                "processor": {
                    "type": "string",
                    "enum": ["lite", "base", "core", "ultra"],
                    "description": "Processor tier — lite (cheapest) → ultra (most thorough)"
                },
                "output_schema": {
                    "type": "object",
                    "description": "JSON schema for the structured output (required)"
                },
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": 10,
                    "maximum": 900,
                    "description": "Max time to wait (default 600)"
                }
            },
            "required": ["input", "processor", "output_schema"]
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
        let output_schema = args
            .get("output_schema")
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: output_schema"))?;

        let mut body = json!({
            "input": input,
            "processor": processor,
            "outputSchema": output_schema,
        });
        if let Some(t) = args.get("timeout_seconds").and_then(|v| v.as_u64()) {
            body["timeoutSeconds"] = json!(t.clamp(10, 900));
        }

        tracing::info!("[parallel_enrich] processor={}", processor);

        match self
            .client
            .post::<EnrichResponse>("/agent-integrations/parallel/enrich", &body)
            .await
        {
            Ok(resp) => {
                let display = match format_enrich_response(EnrichResponse {
                    run_id: resp.run_id.clone(),
                    status: resp.status.clone(),
                    output: resp.output.clone(),
                    cost_usd: resp.cost_usd,
                }) {
                    Ok(display) => display,
                    Err(message) => return Ok(ToolResult::error(message)),
                };
                Ok(ToolResult::success_with_markdown(
                    enrich_payload(&resp, &display),
                    display,
                ))
            }
            Err(e) => Ok(ToolResult::error(format!("Parallel enrich failed: {e}"))),
        }
    }
}
