//! [`CredentialScrubMiddleware`]: scrub credential-shaped secrets out of every
//! tool result before it leaves the tool boundary.

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::{MiddlewareToolOutcome, ToolHandler, ToolMiddleware};
use tinyinference::tool::ToolCall as TaToolCall;

/// Recursively scrub credential-shaped string leaves inside a JSON value.
fn scrub_json_credentials(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => {
            Value::String(crate::agent::harness::credentials::scrub_credentials(&s))
        }
        Value::Array(items) => {
            Value::Array(items.into_iter().map(scrub_json_credentials).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, scrub_json_credentials(v)))
                .collect(),
        ),
        other => other,
    }
}

/// `wrap_tool`: scrub credential-shaped secrets out of every tool result before
/// it leaves the tool boundary (issue #4453). The legacy engine ran
/// `scrub_credentials` over **every** tool output before it entered model
/// context (`engine/tools.rs`); the tinyagents path dropped that call site, so
/// secrets in tool output (env dumps, config reads, API responses, shell output)
/// reached model context, on-disk `session_raw` transcripts, worker-thread
/// mirrors, and the tool-outcome capture sink — violating "Never log secrets or
/// full PII".
///
/// Installed as the **innermost** tool wrap (pushed last), so it observes the
/// RAW tool result first and scrubs it before any outer wrap, the `after_tool`
/// chain (summarization/caps in [`ToolOutputMiddleware`]), the transcript push,
/// or the [`ToolOutcomeCaptureMiddleware`] sink can see the unredacted content.
/// Scrubbing here — rather than inside `execute_openhuman_tool` — covers the
/// parent chat path, sub-agent paths, the persisted transcript, and
/// `ToolCallOutcome` records by construction, since every path runs the same
/// `assemble_turn_harness` seam.
pub(crate) struct CredentialScrubMiddleware;

impl CredentialScrubMiddleware {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ToolMiddleware<()> for CredentialScrubMiddleware {
    fn name(&self) -> &str {
        "credential_scrub"
    }

    async fn wrap_tool(
        &self,
        ctx: &mut RunContext<()>,
        state: &(),
        call: TaToolCall,
        next: ToolHandler<'_, (), ()>,
    ) -> TaResult<MiddlewareToolOutcome> {
        let tool_name = call.name.clone();
        let outcome = next.run(ctx, state, call).await?;
        // `MiddlewareToolOutcome` is `#[non_exhaustive]`; today it only carries a
        // `Result`, but match rather than irrefutable-let so a future variant
        // fails loud instead of silently bypassing scrubbing.
        let mut result = match outcome {
            MiddlewareToolOutcome::Result(result) => result,
            other => return Ok(other),
        };

        let scrubbed_content =
            crate::agent::harness::credentials::scrub_credentials(&result.content);
        if scrubbed_content != result.content {
            tracing::warn!(
                tool = %tool_name,
                "[tinyagents::mw] credential_scrub redacted secret(s) from tool result content"
            );
            result.content = scrubbed_content;
        }

        if let Some(err) = result.error.as_ref() {
            let scrubbed_err = crate::agent::harness::credentials::scrub_credentials(err);
            if &scrubbed_err != err {
                tracing::warn!(
                    tool = %tool_name,
                    "[tinyagents::mw] credential_scrub redacted secret(s) from tool result error"
                );
                result.error = Some(scrubbed_err);
            }
        }

        // Raw JSON payloads (rarely populated on this path) can carry the same
        // secrets — walk their string leaves so a scrubbed `content` isn't
        // undermined by an unredacted `raw` mirror.
        if let Some(raw) = result.raw.take() {
            result.raw = Some(scrub_json_credentials(raw));
        }

        Ok(MiddlewareToolOutcome::Result(result))
    }
}
