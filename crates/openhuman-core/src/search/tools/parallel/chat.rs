//! Chat: `ParallelChatTool` and its response types.

use crate::integrations::IntegrationClient;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ── ParallelChatTool ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    basis: Option<serde_json::Value>,
    #[serde(rename = "costUsd", default)]
    cost_usd: f64,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: ChatMessage,
    #[serde(default, rename = "finish_reason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatMessage {
    #[serde(default)]
    #[allow(dead_code)]
    role: String,
    #[serde(default)]
    content: String,
}

/// AI-powered chat backed by Parallel's web-research models.
pub struct ParallelChatTool {
    client: Arc<IntegrationClient>,
}

impl ParallelChatTool {
    pub fn new(client: Arc<IntegrationClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ParallelChatTool {
    fn name(&self) -> &str {
        "parallel_chat"
    }

    fn description(&self) -> &str {
        "Chat with the web via Parallel's research-grounded chat models. \
         OpenAI-compatible: pass `messages` (system/user/assistant) and a `model` \
         (`speed` cheapest, `lite`, `base`, `core` most capable). Returns the \
         assistant's reply plus optional citation `basis` for research models. \
         Use this for grounded Q&A like \"summarize today's news on …\" or \
         \"what is the current price of BTC?\" — Parallel will browse and cite."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "model": {
                    "type": "string",
                    "enum": ["speed", "lite", "base", "core"],
                    "description": "Model tier — speed (cheapest) → core (most capable)"
                },
                "messages": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string", "enum": ["system", "user", "assistant"] },
                            "content": { "type": "string" }
                        },
                        "required": ["role", "content"]
                    }
                }
            },
            "required": ["model", "messages"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: model"))?;
        let messages = args
            .get("messages")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| anyhow::anyhow!("messages must be a non-empty array"))?;

        tracing::info!(
            "[parallel_chat] model={} messages={}",
            model,
            messages.len()
        );

        let body = json!({ "model": model, "messages": messages });
        match self
            .client
            .post::<ChatResponse>("/agent-integrations/parallel/chat", &body)
            .await
        {
            Ok(resp) => {
                let mut out = String::new();
                if let Some(c) = resp.choices.first() {
                    out.push_str(&c.message.content);
                    if let Some(reason) = &c.finish_reason {
                        out.push_str(&format!("\n\n[finish_reason: {}]", reason));
                    }
                } else {
                    out.push_str("(no choices returned)");
                }
                if let Some(basis) = resp.basis {
                    out.push_str(&format!(
                        "\n\nCitations (basis):\n{}",
                        serde_json::to_string_pretty(&basis).unwrap_or_default()
                    ));
                }
                out.push_str(&format!("\n\nCost: ${:.4}", resp.cost_usd));
                Ok(ToolResult::success(out))
            }
            Err(e) => Ok(ToolResult::error(format!("Parallel chat failed: {e}"))),
        }
    }
}
