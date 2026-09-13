//! Search: `ParallelSearchTool` and its response types.

use crate::integrations::IntegrationClient;
use crate::tools::traits::{Tool, ToolResult};
pub(super) use crate::util::truncate_chars_flagged as truncate_chars;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

// ── Response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct SearchResponse {
    #[serde(rename = "searchId")]
    #[allow(dead_code)]
    pub search_id: String,
    pub results: Vec<SearchResultItem>,
    #[serde(rename = "costUsd")]
    pub cost_usd: f64,
    /// Upstream provider the managed backend resolved this search to
    /// (e.g. "Exa"). Optional: older backends omit it, so this stays
    /// `None` and callers fall back to the managed default. Surfaced to
    /// the UI as the search attribution ("Searched with Exa", #5136).
    /// Aliased so a rename on the backend side keeps deserializing.
    #[serde(
        default,
        alias = "resolvedProvider",
        alias = "searchProvider",
        skip_serializing_if = "Option::is_none"
    )]
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct SearchResultItem {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub publish_date: Option<String>,
    #[serde(default)]
    pub excerpts: Vec<String>,
}

// ── ParallelSearchTool ──────────────────────────────────────────────

/// AI-powered web search via the Parallel API.
pub struct ParallelSearchTool {
    client: Arc<IntegrationClient>,
}

impl ParallelSearchTool {
    pub fn new(client: Arc<IntegrationClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ParallelSearchTool {
    fn name(&self) -> &str {
        "parallel_search"
    }

    fn description(&self) -> &str {
        "AI-powered web search via Parallel. Provide an objective and one or more search \
         queries. Returns relevant results with titles, URLs, and excerpts. \
         Supports modes: 'fast' (quickest), 'one-shot' (balanced), 'agentic' (most thorough). \
         Cost is per request, billed by the backend."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "What you are trying to find or learn"
                },
                "search_queries": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "One or more search queries (1-10)",
                    "minItems": 1,
                    "maxItems": 10
                },
                "mode": {
                    "type": "string",
                    "enum": ["fast", "one-shot", "agentic"],
                    "description": "Search mode (default: fast)",
                    "default": "fast"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results per query (1-50, default 10)",
                    "minimum": 1,
                    "maximum": 50,
                    "default": 10
                },
                "max_characters_per_excerpt": {
                    "type": "integer",
                    "description": "Max characters per excerpt (100-10000, default 500)",
                    "minimum": 100,
                    "maximum": 10000,
                    "default": 500
                }
            },
            "required": ["objective", "search_queries"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let objective = args
            .get("objective")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: objective"))?;

        if objective.trim().is_empty() {
            return Ok(ToolResult::error("objective cannot be empty"));
        }

        let search_queries = args
            .get("search_queries")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: search_queries"))?;

        if search_queries.is_empty() {
            return Ok(ToolResult::error(
                "search_queries must contain at least one query",
            ));
        }

        let mut queries: Vec<&str> = Vec::with_capacity(search_queries.len());
        for (i, v) in search_queries.iter().enumerate() {
            match v.as_str() {
                Some(s) if !s.trim().is_empty() => queries.push(s),
                Some(_) => {
                    return Ok(ToolResult::error(format!(
                        "search_queries[{i}] is an empty string"
                    )));
                }
                None => {
                    return Ok(ToolResult::error(format!(
                        "search_queries[{i}] is not a string"
                    )));
                }
            }
        }

        let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("fast");

        let mut body = json!({
            "objective": objective,
            "searchQueries": queries,
            "mode": mode,
        });

        // Build excerpts config if custom values provided
        let num_results = args.get("num_results").and_then(|v| v.as_u64());
        let max_chars = args
            .get("max_characters_per_excerpt")
            .and_then(|v| v.as_u64());

        if num_results.is_some() || max_chars.is_some() {
            let mut excerpts = json!({});
            if let Some(n) = num_results {
                excerpts["numResults"] = json!(n.clamp(1, 50));
            }
            if let Some(c) = max_chars {
                excerpts["maxCharactersPerExcerpt"] = json!(c.clamp(100, 10000));
            }
            body["excerpts"] = excerpts;
        }

        tracing::info!("[parallel_search] queries={}", queries.len());
        tracing::debug!("[parallel_search] objective={:?}", objective);

        match self
            .client
            .post::<SearchResponse>("/agent-integrations/parallel/search", &body)
            .await
        {
            Ok(resp) => {
                if resp.results.is_empty() {
                    return Ok(ToolResult::success(format!(
                        "No results found for: {}",
                        objective
                    )));
                }

                let mut lines = vec![format!("Search results ({} found):", resp.results.len())];

                for (i, item) in resp.results.iter().enumerate() {
                    lines.push(format!("\n{}. {}", i + 1, item.title));
                    lines.push(format!("   {}", item.url));
                    if let Some(ref date) = item.publish_date {
                        lines.push(format!("   Published: {}", date));
                    }
                    if let Some(excerpt) = item.excerpts.first() {
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
                }

                lines.push(format!("\nCost: ${:.4}", resp.cost_usd));
                Ok(ToolResult::success(lines.join("\n")))
            }
            Err(e) => Ok(ToolResult::error(format!("Parallel search failed: {e}"))),
        }
    }
}
