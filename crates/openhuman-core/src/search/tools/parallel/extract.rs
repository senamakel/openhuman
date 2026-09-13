//! Extract: `ParallelExtractTool` and its response types.

use super::search::truncate_chars;
use crate::integrations::IntegrationClient;
use crate::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub(super) struct ExtractResponse {
    #[serde(rename = "extractId", default)]
    #[allow(dead_code)]
    pub(super) extract_id: String,
    #[serde(default)]
    pub(super) results: Vec<ExtractResultItem>,
    #[serde(default)]
    pub(super) errors: Vec<ExtractError>,
    #[serde(rename = "costUsd", default)]
    pub(super) cost_usd: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExtractResultItem {
    #[serde(default)]
    pub(super) url: String,
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) excerpts: Vec<String>,
    #[serde(default)]
    pub(super) full_content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExtractError {
    #[serde(default)]
    pub(super) url: String,
    #[serde(default)]
    pub(super) error: String,
}

// ── ParallelExtractTool ─────────────────────────────────────────────

/// Extract content from web pages via the Parallel API.
pub struct ParallelExtractTool {
    client: Arc<IntegrationClient>,
}

impl ParallelExtractTool {
    pub fn new(client: Arc<IntegrationClient>) -> Self {
        Self { client }
    }
}

/// Maximum characters of full_content to include per URL in tool output.
const MAX_CONTENT_CHARS: usize = 5000;

#[async_trait]
impl Tool for ParallelExtractTool {
    fn name(&self) -> &str {
        "parallel_extract"
    }

    fn description(&self) -> &str {
        "Extract content from one or more web pages using the Parallel API. \
         Returns page titles, excerpts, and optionally full content. \
         Useful for reading articles, documentation, or structured data from URLs. \
         Cost is per URL, billed by the backend."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "URLs to extract content from (1-20)",
                    "minItems": 1,
                    "maxItems": 20
                },
                "objective": {
                    "type": "string",
                    "description": "What information to focus on when extracting"
                },
                "excerpts": {
                    "type": "boolean",
                    "description": "Include relevant excerpts (default true)",
                    "default": true
                },
                "full_content": {
                    "type": "boolean",
                    "description": "Include full page content (default false)",
                    "default": false
                }
            },
            "required": ["urls"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let urls = args
            .get("urls")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: urls"))?;

        if urls.is_empty() {
            return Ok(ToolResult::error("urls must contain at least one URL"));
        }

        let mut url_strings: Vec<&str> = Vec::with_capacity(urls.len());
        for (i, v) in urls.iter().enumerate() {
            match v.as_str() {
                Some(s) if !s.trim().is_empty() => url_strings.push(s),
                Some(_) => {
                    return Ok(ToolResult::error(format!("urls[{i}] is an empty string")));
                }
                None => {
                    return Ok(ToolResult::error(format!("urls[{i}] is not a string")));
                }
            }
        }

        let objective = args.get("objective").and_then(|v| v.as_str());
        let excerpts = args
            .get("excerpts")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let full_content = args
            .get("full_content")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut body = json!({
            "urls": url_strings,
            "excerpts": excerpts,
            "fullContent": full_content,
        });

        if let Some(obj) = objective {
            body["objective"] = json!(obj);
        }

        tracing::info!("[parallel_extract] urls={}", url_strings.len());

        match self
            .client
            .post::<ExtractResponse>("/agent-integrations/parallel/extract", &body)
            .await
        {
            Ok(resp) => {
                let mut lines = Vec::new();

                for (i, item) in resp.results.iter().enumerate() {
                    let title = item.title.as_deref().unwrap_or("(no title)");
                    lines.push(format!("\n{}. {} — {}", i + 1, title, item.url));

                    for excerpt in &item.excerpts {
                        let text = excerpt.trim();
                        if !text.is_empty() {
                            let (slice, was_truncated) = truncate_chars(text, 500);
                            let truncated = if was_truncated {
                                format!("{slice}...")
                            } else {
                                slice.to_string()
                            };
                            lines.push(format!("   {}", truncated));
                        }
                    }

                    if let Some(ref content) = item.full_content {
                        let content = content.trim();
                        if !content.is_empty() {
                            let (slice, was_truncated) = truncate_chars(content, MAX_CONTENT_CHARS);
                            let truncated = if was_truncated {
                                format!(
                                    "{}... [truncated, {} chars total]",
                                    slice,
                                    content.chars().count()
                                )
                            } else {
                                slice.to_string()
                            };
                            lines.push(format!("   Content:\n   {}", truncated));
                        }
                    }
                }

                if !resp.errors.is_empty() {
                    lines.push("\nErrors:".to_string());
                    for err in &resp.errors {
                        lines.push(format!("  {} — {}", err.url, err.error));
                    }
                }

                if lines.is_empty() {
                    Ok(ToolResult::success(format!(
                        "No content extracted from the provided URLs.\nCost: ${:.4}",
                        resp.cost_usd
                    )))
                } else {
                    lines.push(format!("\nCost: ${:.4}", resp.cost_usd));
                    Ok(ToolResult::success(lines.join("\n")))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Parallel extract failed: {e}"))),
        }
    }
}
