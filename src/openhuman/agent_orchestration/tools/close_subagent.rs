//! Tool: `close_subagent` - retire a reusable durable sub-agent session.

use crate::openhuman::agent::harness::fork_context::current_parent;
use crate::openhuman::agent_orchestration::subagent_sessions::SubagentSessionStore;
use crate::openhuman::agent_orchestration::{running_subagents, subagent_sessions};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct CloseSubagentTool;

impl CloseSubagentTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CloseSubagentTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CloseSubagentTool {
    fn name(&self) -> &str {
        "close_subagent"
    }

    fn description(&self) -> &str {
        "Close a reusable sub-agent session so future delegation creates a fresh worker. \
         If that session is currently running, it is cancelled first."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["subagent_session_id"],
            "properties": {
                "subagent_session_id": {
                    "type": "string",
                    "description": "Durable subagent_session_id returned by reusable async delegation."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let subagent_session_id = args
            .get("subagent_session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if subagent_session_id.is_empty() {
            return Ok(ToolResult::error(
                "close_subagent: `subagent_session_id` is required",
            ));
        }
        let parent = match current_parent() {
            Some(parent) => parent,
            None => {
                return Ok(ToolResult::error(
                    "close_subagent called outside of an agent turn",
                ));
            }
        };
        let cancelled =
            running_subagents::cancel_by_session(&subagent_session_id, &parent.session_id)
                .is_some();
        let store = SubagentSessionStore::new(parent.workspace_dir.clone());
        match subagent_sessions::close(&store, &subagent_session_id) {
            Ok(closed) => {
                log::info!(
                    "[subagent_reuse] close subagent_session_id={} parent_session={} closed={} cancelled_running={}",
                    subagent_session_id,
                    parent.session_id,
                    closed,
                    cancelled
                );
                Ok(ToolResult::success(format!(
                    "Closed reusable sub-agent session `{subagent_session_id}` (closed={closed}, cancelled_running={cancelled})."
                )))
            }
            Err(err) => Ok(ToolResult::error(format!(
                "close_subagent: failed to update sub-agent session: {err}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_session_id_is_rejected() {
        let res = CloseSubagentTool::new().execute(json!({})).await.unwrap();
        assert!(res.is_error);
        assert!(res.output().contains("subagent_session_id"));
    }
}
