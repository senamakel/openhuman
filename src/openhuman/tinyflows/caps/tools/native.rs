//! The `oh:` namespace — native OpenHuman tools.
//!
//! A "Tool" node in a flow, as opposed to an "App action" node. The slug names
//! a tool in the same agent registry the assistant itself uses, so a flow
//! reaches the full toolset (search, media generation, file, shell, …) rather
//! than a flow-specific subset.
//!
//! Composio's curation gate does not apply here — an `oh:` slug is not a
//! Composio action — but the autonomy-tier and approval gates do, and for the
//! same reason they apply to the assistant: a flow is another way to reach the
//! same effects.

use async_trait::async_trait;
use serde_json::Value;
use tinyflows::error::{EngineError, Result};

use super::{ToolBackend, ToolCallCtx, NATIVE_TOOL_PREFIX};
use crate::openhuman::config::Config;

/// Dispatches `oh:<tool_name>` to the agent tool registry.
pub(crate) struct NativeToolBackend;

#[async_trait]
impl ToolBackend for NativeToolBackend {
    fn name(&self) -> &'static str {
        "native"
    }

    fn prefix(&self) -> &'static str {
        NATIVE_TOOL_PREFIX
    }

    async fn invoke(
        &self,
        ctx: &ToolCallCtx<'_>,
        slug: &str,
        args: Value,
        _conn: Option<&str>,
    ) -> Result<Value> {
        let tool_name = slug.strip_prefix(NATIVE_TOOL_PREFIX).unwrap_or(slug).trim();
        if tool_name.is_empty() {
            return Err(EngineError::Capability(
                "tool_call node: native tool slug is empty (expected `oh:<tool_name>`)".to_string(),
            ));
        }

        let class =
            crate::openhuman::runtime_node::ops::classify_tool_call(ctx.config, tool_name, &args)
                .map_err(EngineError::Capability)?;
        let tier_decision = super::super::enforce_node_tier_gate(ctx.security, class, "tool_call")?;
        let summary = crate::openhuman::approval::summarize_action(tool_name, &args);
        let redacted = crate::openhuman::approval::redact_args(&args);
        let (outcome, _request_id) =
            super::super::gate_call_for_tier(tier_decision, tool_name, &summary, redacted).await;
        if let crate::openhuman::approval::GateOutcome::Deny { reason } = outcome {
            return Err(EngineError::Capability(reason));
        }
        tracing::debug!(
            target: "flows",
            %tool_name,
            ?class,
            ?tier_decision,
            "[flows] tool_call: dispatching NATIVE OpenHuman tool"
        );
        let outcome =
            crate::openhuman::runtime_node::ops::execute_tool(ctx.config, tool_name, args, false)
                .await
                .map_err(EngineError::Capability)?;
        super::super::reject_failed_native_tool_result(slug, &outcome.result)
            .map(|_| super::super::native_tool_payload(&outcome.result))
    }

    /// Native tools have no published arg schema to check against, so a dry run
    /// validates nothing here rather than inventing a check the real path does
    /// not perform.
    async fn preflight(&self, _config: &Config, _slug: &str, _args: &Value) -> Result<()> {
        Ok(())
    }
}
