//! `TavilySearchTool`: web / news / finance search via `POST /search`.

use super::client::TavilyClient;
use super::types::{copy_bool, copy_domain_filter, copy_string, non_empty, TavilySearchResponse};
use crate::tools::traits::{Tool, ToolCallOptions, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Web / news / finance search via the Tavily API (`POST /search`).
pub struct TavilySearchTool {
    tool_name: &'static str,
    client: TavilyClient,
}

impl TavilySearchTool {
    pub fn new(
        api_key: Option<String>,
        api_url: Option<String>,
        max_results: usize,
        timeout_secs: u64,
    ) -> Self {
        Self {
            tool_name: "tavily_search",
            client: TavilyClient::new(api_key, api_url, max_results, timeout_secs),
        }
    }

    /// Same tool under the canonical `web_search_tool` slot, so selecting Tavily
    /// satisfies the agent's generic "search the web" affordance.
    pub fn new_web_search_tool(
        api_key: Option<String>,
        api_url: Option<String>,
        max_results: usize,
        timeout_secs: u64,
    ) -> Self {
        Self {
            tool_name: "web_search_tool",
            client: TavilyClient::new(api_key, api_url, max_results, timeout_secs),
        }
    }

    pub(crate) fn build_body(&self, args: &Value, query: &str) -> Value {
        let mut body = json!({
            "query": query,
            "max_results": self.client.requested_results(args),
        });
        copy_string(args, "search_depth", &mut body);
        copy_string(args, "topic", &mut body);
        copy_string(args, "time_range", &mut body);
        copy_string(args, "start_date", &mut body);
        copy_string(args, "end_date", &mut body);
        copy_bool(args, "include_answer", &mut body);
        copy_bool(args, "include_raw_content", &mut body);
        copy_bool(args, "include_images", &mut body);
        copy_domain_filter(args, "include_domains", &mut body);
        copy_domain_filter(args, "exclude_domains", &mut body);
        body
    }
}

#[async_trait]
impl Tool for TavilySearchTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn description(&self) -> &str {
        "Search the web with Tavily. Returns ranked pages with URLs, titles, and \
         snippets. Supports general, news, and finance topics; search-depth levels; \
         a publish/update time range or explicit start/end dates; domain include/ \
         exclude filters; and an optional LLM-generated answer, cleaned page content, \
         or image links."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query. Write it in the language you want results in."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default from config, max 20)."
                },
                "search_depth": {
                    "type": "string",
                    "enum": ["basic", "advanced", "fast", "ultra-fast"],
                    "description": "Latency vs. relevance tradeoff. Advanced costs more and gives higher-relevance results; ultra-fast is the quickest."
                },
                "topic": {
                    "type": "string",
                    "enum": ["general", "news", "finance"],
                    "description": "Category of the search. Use 'news' for real-time coverage, 'finance' for financial data."
                },
                "time_range": {
                    "type": "string",
                    "enum": ["day", "week", "month", "year"],
                    "description": "Only results published or updated within this period back from today. Alternative to start_date/end_date."
                },
                "start_date": {
                    "type": "string",
                    "description": "Only results published/updated on or after this YYYY-MM-DD date."
                },
                "end_date": {
                    "type": "string",
                    "description": "Only results published/updated on or before this YYYY-MM-DD date."
                },
                "include_answer": {
                    "type": "boolean",
                    "description": "Also return an LLM-generated concise answer to the query."
                },
                "include_raw_content": {
                    "type": "boolean",
                    "description": "Also return cleaned page content for each result, bounded by OpenHuman to protect the agent context (larger responses)."
                },
                "include_images": {
                    "type": "boolean",
                    "description": "Also return query-related images in the response."
                },
                "include_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only return results from these domains."
                },
                "exclude_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Exclude results from these domains."
                }
            },
            "required": ["query"]
        })
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    async fn execute_with_options(
        &self,
        args: Value,
        options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        if let Some(blocked) = self.client.local_only_block() {
            return Ok(blocked);
        }

        let query = non_empty(args.get("query").and_then(Value::as_str))
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: query"))?;

        let limit = self.client.requested_results(&args);
        let body = self.build_body(&args, &query);
        let value = self.client.post("search", body).await?;
        let parsed: TavilySearchResponse = serde_json::from_value(value).map_err(|e| {
            tracing::warn!("[tavily] failed to parse search response: {e}");
            anyhow::anyhow!("Failed to parse Tavily search response: {e}")
        })?;

        tracing::debug!(result_count = parsed.results.len(), "[tavily] search ok");

        // A Tavily-generated answer is the provider's own synthesis of the
        // results; surface it when requested so the agent does not re-answer
        // from the scraps. Gate on the requested flag — the response can carry
        // an answer the agent never asked for.
        let include_answer = args
            .get("include_answer")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer = if include_answer {
            non_empty(parsed.answer.as_deref())
        } else {
            None
        };
        let include_raw_content = args
            .get("include_raw_content")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let include_images = args
            .get("include_images")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let plain = self.client.render_plain(
            &parsed.results,
            &parsed.images,
            &query,
            limit,
            include_raw_content,
            include_images,
        );
        let markdown = self.client.render_markdown(
            &parsed.results,
            &parsed.images,
            &query,
            limit,
            answer.as_deref(),
            include_raw_content,
            include_images,
        );
        let output = match answer {
            Some(answer) => format!("Answer: {answer}\n\n{plain}"),
            None => plain,
        };

        let mut result = ToolResult::success(output);
        if options.prefer_markdown {
            result.markdown_formatted = Some(markdown);
        }
        Ok(result)
    }
}
