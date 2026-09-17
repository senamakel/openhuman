//! [`ComposioClient`]'s tool-discovery and execution surface: `list_tools`,
//! the `execute_tool*` family (including the post-OAuth readiness retry),
//! and the shared `post_execute_tool` HTTP call.

use std::time::Duration;

use anyhow::Result;
use serde_json::json;

use super::super::types::{ComposioExecuteResponse, ComposioToolsResponse};
use super::connections::ComposioClient;

const POST_OAUTH_ACTION_RETRY_DELAY: Duration = Duration::from_secs(10);
/// Literal error fragments Composio's gateway emits during the post-OAuth
/// readiness gap. Matching is case-insensitive and substring-based so
/// trailing punctuation or wrapper text from the gateway does not silently
/// disable the retry.
const POST_OAUTH_AUTH_ERROR_STRINGS: &[&str] = &["connection error, try to authenticate"];

impl ComposioClient {
    pub async fn list_tools(
        &self,
        toolkits: Option<&[String]>,
        tags: Option<&[String]>,
    ) -> Result<ComposioToolsResponse> {
        let mut params: Vec<String> = Vec::new();
        if let Some(list) = toolkits {
            let joined = list
                .iter()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(|t| urlencoding::encode(t).into_owned())
                .collect::<Vec<_>>()
                .join(",");
            if !joined.is_empty() {
                params.push(format!("toolkits={joined}"));
            }
        }
        if let Some(list) = tags {
            let joined = list
                .iter()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(|t| urlencoding::encode(t).into_owned())
                .collect::<Vec<_>>()
                .join(",");
            if !joined.is_empty() {
                params.push(format!("tags={joined}"));
            }
        }
        let path = if params.is_empty() {
            "/agent-integrations/composio/tools".to_string()
        } else {
            format!("/agent-integrations/composio/tools?{}", params.join("&"))
        };
        tracing::debug!(path = %path, "[composio] list_tools");
        self.inner.get::<ComposioToolsResponse>(&path).await
    }

    // ── Execute ─────────────────────────────────────────────────────

    /// `POST /agent-integrations/composio/execute` — run a Composio
    /// action and return the provider result + cost.
    pub async fn execute_tool(
        &self,
        tool: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<ComposioExecuteResponse> {
        self.execute_tool_with_connection(tool, arguments, None)
            .await
    }

    /// `POST /agent-integrations/composio/execute` — run a Composio action
    /// against one specific connected account.
    ///
    /// `connection_id = None` preserves [`Self::execute_tool`]'s ambient-account
    /// behavior. A non-empty id is forwarded as `connectionId`; the backend
    /// verifies that the authenticated user owns it before dispatching.
    pub async fn execute_tool_with_connection(
        &self,
        tool: &str,
        arguments: Option<serde_json::Value>,
        connection_id: Option<&str>,
    ) -> Result<ComposioExecuteResponse> {
        let tool = tool.trim();
        if tool.is_empty() {
            anyhow::bail!("composio.execute_tool: tool slug must not be empty");
        }
        // Egress spine (privacy epic S2, #4436): a Composio tool call ships the
        // (already-normalized) arguments to the third-party provider — disclose
        // the transfer before the round-trip. S4 will add an approval arm here.
        let egress = crate::security::egress::EgressDescriptor::composio(tool);
        // Local-only enforcement (privacy epic S7, #4441): refuse the external
        // tool call under LocalOnly BEFORE disclosing or sending it.
        crate::security::egress::enforce_egress(&egress)?;
        crate::security::egress::emit_external_transfer(egress);
        // PR #1827 routes all execute-side argument normalization
        // (including the bare-date → RFC 3339 fix #1802 brought to
        // `normalize_calendar_query_args` on `main`) through the
        // centralized `prepare_execute_arguments` helper. The helper
        // covers the same calendar query case and is the shared entry
        // point for `composio_execute`, per-action tools, and direct-
        // mode dispatch.
        let arguments = super::super::execute_prepare::prepare_execute_arguments(tool, arguments)
            .map_err(anyhow::Error::msg)?;
        let connection_id = connection_id.map(str::trim).filter(|id| !id.is_empty());
        tracing::debug!(
            tool = %tool,
            connection_id = ?connection_id,
            "[composio] execute_tool"
        );
        let mut body = json!({ "tool": tool, "arguments": arguments });
        if let Some(connection_id) = connection_id {
            body["connectionId"] = json!(connection_id);
        }
        let mut resp = self
            .execute_tool_with_post_oauth_retry(tool, &body, POST_OAUTH_ACTION_RETRY_DELAY)
            .await?;
        if !resp.successful {
            if let Some(ref err) = resp.error {
                resp.error = Some(super::super::error_mapping::format_provider_error(
                    tool, err,
                ));
            }
        }
        Ok(resp)
    }

    /// `POST /agent-integrations/composio/execute` — single, non-retrying
    /// HTTP round-trip. Use this when the caller owns the retry loop
    /// (e.g. `auth_retry`) to avoid double-retry. In particular,
    /// [`super::super::auth_retry::execute_with_auth_retry`] uses this entry
    /// point so its `must retry exactly once` contract still holds
    /// after PR #1707 introduced the inner retry.
    pub(crate) async fn execute_tool_once(
        &self,
        tool: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<ComposioExecuteResponse> {
        let tool = tool.trim();
        if tool.is_empty() {
            anyhow::bail!("composio.execute_tool_once: tool slug must not be empty");
        }
        // Egress spine (privacy epic S2, #4436): see `execute_tool`. This is the
        // caller-owns-retry entry point (e.g. `auth_retry`), disjoint from
        // `execute_tool`, so each logical tool call emits exactly once.
        let egress = crate::security::egress::EgressDescriptor::composio(tool);
        // Local-only enforcement (privacy epic S7, #4441): same gate as
        // `execute_tool` — this disjoint entry point must block too.
        crate::security::egress::enforce_egress(&egress)?;
        crate::security::egress::emit_external_transfer(egress);
        let arguments = super::super::execute_prepare::prepare_execute_arguments(tool, arguments)
            .map_err(anyhow::Error::msg)?;
        tracing::debug!(tool = %tool, "[composio] execute_tool_once (no built-in retry)");
        let body = json!({ "tool": tool, "arguments": arguments });
        let result = self.post_execute_tool(&body).await;
        match &result {
            Ok(resp) => tracing::debug!(
                tool = %tool,
                successful = resp.successful,
                has_error = resp.error.is_some(),
                "[composio] execute_tool_once completed"
            ),
            Err(err) => tracing::warn!(
                tool = %tool,
                error = %err,
                "[composio] execute_tool_once failed"
            ),
        }
        result.map_err(|e| {
            anyhow::Error::msg(super::super::error_mapping::remap_transport_error(
                tool,
                &e.to_string(),
            ))
        })
    }

    pub(crate) async fn execute_tool_with_post_oauth_retry(
        &self,
        tool: &str,
        body: &serde_json::Value,
        retry_delay: Duration,
    ) -> Result<ComposioExecuteResponse> {
        tracing::debug!(
            tool = %tool,
            retry_delay_ms = retry_delay.as_millis() as u64,
            attempt = 1u8,
            "[composio] execute_tool_with_post_oauth_retry attempt"
        );
        let first = self.post_execute_tool(body).await?;
        let should_retry = is_post_oauth_auth_readiness_error(&first);
        tracing::debug!(
            tool = %tool,
            attempt = 1u8,
            successful = first.successful,
            has_error = first.error.is_some(),
            should_retry,
            "[composio] execute_tool_with_post_oauth_retry branch decision"
        );
        if !should_retry {
            return Ok(first);
        }

        tracing::warn!(
            tool = %tool,
            retry_delay_ms = retry_delay.as_millis() as u64,
            "[composio] action returned post-OAuth auth-readiness error; retrying once"
        );
        if !retry_delay.is_zero() {
            tokio::time::sleep(retry_delay).await;
        }
        tracing::debug!(
            tool = %tool,
            retry_delay_ms = retry_delay.as_millis() as u64,
            attempt = 2u8,
            "[composio] execute_tool_with_post_oauth_retry retry dispatch"
        );
        let retry = self.post_execute_tool(body).await;
        match &retry {
            Ok(resp) => tracing::debug!(
                tool = %tool,
                attempt = 2u8,
                successful = resp.successful,
                has_error = resp.error.is_some(),
                "[composio] execute_tool_with_post_oauth_retry retry completed"
            ),
            Err(err) => tracing::debug!(
                tool = %tool,
                attempt = 2u8,
                error = %err,
                "[composio] execute_tool_with_post_oauth_retry retry failed"
            ),
        }
        retry
    }

    async fn post_execute_tool(&self, body: &serde_json::Value) -> Result<ComposioExecuteResponse> {
        self.inner
            .post::<ComposioExecuteResponse>("/agent-integrations/composio/execute", body)
            .await
    }
}

pub(super) fn is_post_oauth_auth_readiness_error(resp: &ComposioExecuteResponse) -> bool {
    if resp.successful {
        return false;
    }
    let Some(error) = resp.error.as_deref() else {
        return false;
    };
    let normalized = error.trim().to_ascii_lowercase();
    POST_OAUTH_AUTH_ERROR_STRINGS
        .iter()
        .any(|needle| normalized.contains(needle))
}
