//! `TavilyExtractTool`: full page content retrieval via `POST /extract`.

use super::client::TavilyClient;
use super::types::{copy_string, non_empty, TavilyExtractResponse, TavilyExtractResult};
use crate::search::tools::tavily::types::escape_link_destination;
use crate::search::tools::tavily::types::escape_link_text;
use crate::tools::traits::{Tool, ToolCallOptions, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Retrieve full page contents for a list of URLs (`POST /extract`).
pub struct TavilyExtractTool {
    pub(super) client: TavilyClient,
}

impl TavilyExtractTool {
    pub fn new(
        api_key: Option<String>,
        api_url: Option<String>,
        max_results: usize,
        timeout_secs: u64,
    ) -> Self {
        // Extraction transfers the full cleaned page, which can be hundreds of
        // kilobytes or more over a slow link, while search only moves small
        // excerpts. A hot folder of small search responses needs 15s; a single
        // large page commonly does not. Tavily itself allows up to 60s per
        // request, so the extract budget must never be smaller than that cap —
        // otherwise a large page dies to `operation timed out` after the server
        // already answered 200.
        Self {
            client: TavilyClient::new(api_key, api_url, max_results, timeout_secs.max(60)),
        }
    }

    /// Accept either a `urls` array or a single `url` string, so the agent can
    /// call this the obvious way for the one-document case.
    pub(crate) fn collect_urls(args: &Value) -> anyhow::Result<Vec<String>> {
        let urls: Vec<String> = match args.get("urls") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|v| non_empty(v.as_str()))
                .collect::<Vec<_>>(),
            Some(Value::String(single)) => non_empty(Some(single)).into_iter().collect(),
            _ => non_empty(args.get("url").and_then(Value::as_str))
                .into_iter()
                .collect(),
        };
        if urls.is_empty() {
            anyhow::bail!("Missing required parameter: urls (a non-empty list of page URLs)");
        }
        if urls.len() > 20 {
            anyhow::bail!(
                "Tavily extract accepts at most 20 URLs per call (got {})",
                urls.len()
            );
        }
        Ok(urls)
    }

    fn render_plain(&self, results: &[TavilyExtractResult]) -> String {
        if results.is_empty() {
            return "No Tavily extraction results.".to_string();
        }
        let mut lines = Vec::new();
        for (i, item) in results.iter().enumerate() {
            lines.push(format!("{}. {}", i + 1, item.url.trim()));
            match non_empty(item.raw_content.as_deref()) {
                Some(content) => {
                    let truncated = crate::util::truncate_with_ellipsis(&content, 8_000);
                    lines.push(format!("   {truncated}"));
                }
                None => lines.push("   (no content extracted)".to_string()),
            }
        }
        lines.join("\n")
    }

    fn render_markdown(&self, results: &[TavilyExtractResult]) -> String {
        if results.is_empty() {
            return "_No Tavily extraction results._".to_string();
        }
        let mut out = String::from("# Tavily extraction\n");
        for item in results {
            out.push_str(&format!(
                "\n## [{}]({})\n",
                escape_link_text(item.url.trim()),
                escape_link_destination(&item.url)
            ));
            match non_empty(item.raw_content.as_deref()) {
                Some(content) => {
                    let truncated = crate::util::truncate_with_suffix(&content, 8_000, "...");
                    out.push_str(&truncated);
                    out.push('\n');
                }
                None => out.push_str("_No content extracted._\n"),
            }
        }
        out
    }
}

#[async_trait]
impl Tool for TavilyExtractTool {
    fn name(&self) -> &str {
        "tavily_extract"
    }

    fn description(&self) -> &str {
        "Extract cleaned content from one or more web pages using Tavily. Returns \
         markdown or plain text, bounded to 8,000 characters per URL."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "urls": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": 20,
                    "description": "The page URLs to extract content from (up to 20)."
                },
                "format": {
                    "type": "string",
                    "enum": ["markdown", "text"],
                    "description": "Format of the extracted content. Defaults to markdown."
                },
                "extract_depth": {
                    "type": "string",
                    "enum": ["basic", "advanced"],
                    "description": "basic is faster and cheaper; advanced also captures tables and embedded content."
                }
            },
            "required": ["urls"]
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

        let urls = Self::collect_urls(&args)?;

        let mut body = json!({
            "urls": urls,
            "format": "markdown",
        });
        copy_string(&args, "format", &mut body);
        copy_string(&args, "extract_depth", &mut body);

        let value = self.client.post("extract", body).await?;
        let parsed: TavilyExtractResponse = serde_json::from_value(value).map_err(|e| {
            tracing::warn!("[tavily] failed to parse extract response: {e}");
            anyhow::anyhow!("Failed to parse Tavily extract response: {e}")
        })?;

        tracing::debug!(result_count = parsed.results.len(), "[tavily] extract ok");

        let failed = parsed.failed_results.len();
        if failed > 0 {
            tracing::warn!(failed, "[tavily] extraction failures");
        }
        if parsed.results.is_empty() && failed > 0 {
            return Ok(ToolResult::error(format!(
                "Tavily could not extract any of the requested URLs ({failed} failed)."
            )));
        }

        let plain = self.render_plain(&parsed.results);
        let mut markdown = self.render_markdown(&parsed.results);
        let output = if failed == 0 {
            plain
        } else {
            // Report failures as a trailing line without echoing the remote
            // error verbatim into the transcript.
            markdown.push_str(&format!(
                "\n> {failed} URL(s) could not be extracted by Tavily.\n"
            ));
            format!("{plain}\n\n({failed} URL(s) could not be extracted by Tavily)")
        };

        let mut result = ToolResult::success(output);
        if options.prefer_markdown {
            result.markdown_formatted = Some(markdown);
        }
        Ok(result)
    }
}
