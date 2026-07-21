//! The concrete openhuman implementation of the serve [`HostPorts`] surface
//! (plan §3.2 — "the substance of the flavor").
//!
//! * `inference` routes onto the existing per-role provider factory: the serve
//!   tier (`orchestrator`/`reasoning`/`compress`) maps to an openhuman model
//!   role via [`role_for_model_tier`]/[`provider_for_role`], the same routing
//!   BYOK / local / managed-backend all already flow through.
//! * `tools` exposes a SMALL curated allowlist of **read-only** tools drawn
//!   from the full local surface (`runtime_node::ops::build_runtime_tools`,
//!   itself `tools::ops::all_tools_with_runtime`). Every result passes through
//!   unchanged; write/execute tools are refused for this draft.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, warn};

use super::ports::{HostPorts, PortError};
use super::types::{InferenceCall, InferenceResult, ToolSpec, Usage};
use crate::openhuman::config::Config;
use crate::openhuman::inference::provider::{
    create_chat_model_with_model_id, provider_for_role, role_for_model_tier,
};
use crate::openhuman::tools::PermissionLevel;
use tinyagents::harness::message::Message;
use tinyagents::harness::model::ModelRequest;

/// The curated read-only tool names serve may invoke this draft. The port also
/// enforces `PermissionLevel::ReadOnly` at call time, so an allowlisted name
/// that ever gained a write action is still refused.
const CURATED_READ_ONLY_TOOLS: &[&str] = &[
    "file_read",
    "read_diff",
    "list",
    "glob",
    "grep",
    "web_fetch",
];

/// Adapter holding the config snapshot the port routing needs.
pub struct OpenhumanHostPorts {
    config: Arc<Config>,
}

impl OpenhumanHostPorts {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }

    /// Map a serve inference tier onto an openhuman model role.
    ///
    /// `orchestrator` → the agentic (tool-using) role, `reasoning` → the
    /// reasoning role, `compress` → the summarization role; anything else
    /// falls back through `role_for_model_tier`'s own default (chat).
    fn role_for_tier(tier: &str) -> &'static str {
        match tier {
            "orchestrator" => "agentic",
            "reasoning" => "reasoning",
            "compress" => "summarization",
            other => role_for_model_tier(other),
        }
    }

    /// The curated read-only tools drawn from the full local surface: the
    /// intersection of [`CURATED_READ_ONLY_TOOLS`] with the built runtime tools,
    /// filtered again on `PermissionLevel::ReadOnly` so an allowlisted name that
    /// ever gained a write action is dropped. Both the `hello` advertisement
    /// ([`Self::tool_specs`]) and the `tools.invoke` handler
    /// ([`Self::invoke_tool`]) resolve tools through this one path, keeping the
    /// advertised set and the invocable set identical.
    fn curated_read_only_tools(
        &self,
    ) -> Result<Vec<Box<dyn crate::openhuman::tools::Tool>>, String> {
        let tools = crate::openhuman::runtime_node::ops::build_runtime_tools(&self.config)?;
        Ok(tools
            .into_iter()
            .filter(|tool| CURATED_READ_ONLY_TOOLS.contains(&tool.name()))
            .filter(|tool| tool.permission_level() == PermissionLevel::ReadOnly)
            .collect())
    }
}

#[async_trait]
impl HostPorts for OpenhumanHostPorts {
    fn tool_specs(&self) -> Vec<ToolSpec> {
        let tools = match self.curated_read_only_tools() {
            Ok(tools) => tools,
            Err(error) => {
                warn!(
                    "[medulla_local] building tool surface for hello advertisement failed: {error}"
                );
                return Vec::new();
            }
        };
        let specs: Vec<ToolSpec> = tools
            .iter()
            .map(|tool| ToolSpec {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect();
        debug!(
            count = specs.len(),
            "[medulla_local] advertising curated read-only tools in hello"
        );
        specs
    }

    async fn invoke_inference(&self, call: InferenceCall) -> Result<InferenceResult, PortError> {
        let role = Self::role_for_tier(&call.tier);
        debug!(
            tier = %call.tier,
            role,
            provider = %provider_for_role(role, &self.config),
            messages = call.messages.len(),
            "[medulla_local] inference callback routing"
        );

        let (model, model_id) =
            create_chat_model_with_model_id(role, &self.config, 0.2).map_err(|error| {
                PortError::internal(format!("model init for role `{role}`: {error}"))
            })?;

        let messages: Vec<Message> = call
            .messages
            .iter()
            .map(|message| match message.role.as_str() {
                "system" => Message::system(message.content.clone()),
                "assistant" => Message::assistant(message.content.clone()),
                _ => Message::user(message.content.clone()),
            })
            .collect();

        let response = model
            .invoke(&(), ModelRequest::new(messages).with_temperature(0.2))
            .await
            .map_err(|error| PortError::internal(format!("inference invoke failed: {error}")))?;

        Ok(InferenceResult {
            content: response.text(),
            reasoning_content: None,
            model: model_id,
            tool_calls: Vec::new(),
            usage: Usage::default(),
        })
    }

    async fn invoke_tool(&self, name: &str, args: Value) -> Result<Value, PortError> {
        if !CURATED_READ_ONLY_TOOLS.contains(&name) {
            return Err(PortError::port_unavailable(format!(
                "tool `{name}` is not on the curated read-only allowlist (draft)"
            )));
        }

        // Resolve through the same curated (allowlisted + read-only) surface the
        // `hello` advertisement is built from, so a tool the model was told about
        // is exactly a tool this handler will run. The `curated_read_only_tools`
        // filter already drops anything not read-only; the explicit re-check
        // below is defence in depth against future drift.
        let tools = self
            .curated_read_only_tools()
            .map_err(|error| PortError::internal(format!("building tool surface: {error}")))?;
        let tool = tools
            .into_iter()
            .find(|tool| tool.name() == name)
            .ok_or_else(|| {
                PortError::port_unavailable(format!("tool `{name}` not present in local surface"))
            })?;

        // Defence in depth: refuse anything that is not read-only even if the
        // allowlist and the tool registry ever drift.
        if tool.permission_level() != PermissionLevel::ReadOnly {
            warn!(
                name,
                "[medulla_local] refusing non-read-only tool over medulla port"
            );
            return Err(PortError::port_unavailable(format!(
                "tool `{name}` is not read-only; refused for this draft"
            )));
        }

        let result = tool
            .execute(args)
            .await
            .map_err(|error| PortError::internal(format!("tool `{name}` failed: {error}")))?;

        // Pass the ToolResult through unchanged, adapting only the error-flag
        // key to the medulla wire shape (`isError`, §5.2).
        Ok(json!({
            "content": serde_json::to_value(&result.content).unwrap_or_else(|_| json!([])),
            "isError": result.is_error,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_maps_onto_expected_roles() {
        assert_eq!(OpenhumanHostPorts::role_for_tier("orchestrator"), "agentic");
        assert_eq!(OpenhumanHostPorts::role_for_tier("reasoning"), "reasoning");
        assert_eq!(
            OpenhumanHostPorts::role_for_tier("compress"),
            "summarization"
        );
        // Unknown tiers fall back through the factory's own default.
        assert_eq!(OpenhumanHostPorts::role_for_tier("mystery"), "chat");
    }

    #[test]
    fn curated_allowlist_is_read_only_by_name() {
        // Sanity: the curated set never contains an obviously-mutating tool.
        for forbidden in ["file_write", "edit", "apply_patch", "git_operations"] {
            assert!(!CURATED_READ_ONLY_TOOLS.contains(&forbidden));
        }
    }
}
