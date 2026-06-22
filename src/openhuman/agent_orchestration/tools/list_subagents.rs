//! Tool: `list_subagents` - inspect reusable sub-agent sessions for this parent.

use crate::openhuman::agent::harness::fork_context::current_parent;
use crate::openhuman::agent_orchestration::subagent_sessions::{self, SubagentSessionStore};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct ListSubagentsTool;

impl ListSubagentsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListSubagentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ListSubagentsTool {
    fn name(&self) -> &str {
        "list_subagents"
    }

    fn description(&self) -> &str {
        "List active or reusable durable sub-agents owned by the current parent thread."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": [],
            "properties": {}
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let parent = match current_parent() {
            Some(parent) => parent,
            None => {
                return Ok(ToolResult::error(
                    "list_subagents called outside of an agent turn",
                ));
            }
        };
        let parent_thread_id =
            crate::openhuman::inference::provider::thread_context::current_thread_id();
        let store = SubagentSessionStore::new(parent.workspace_dir.clone());
        match subagent_sessions::list_for_parent(
            &store,
            &parent.session_id,
            parent_thread_id.as_deref(),
        ) {
            Ok(sessions) => {
                log::debug!(
                    "[subagent_reuse] list parent_thread_id={} parent_session={} count={}",
                    parent_thread_id.as_deref().unwrap_or("none"),
                    parent.session_id,
                    sessions.len()
                );
                Ok(ToolResult::success(format!(
                    "[subagent_sessions]\n{}\n[/subagent_sessions]",
                    serde_json::to_string_pretty(&sessions).unwrap_or_else(|_| "[]".to_string())
                )))
            }
            Err(err) => Ok(ToolResult::error(format!(
                "list_subagents: failed to read sub-agent sessions: {err}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_no_required_fields() {
        let schema = ListSubagentsTool::new().parameters_schema();
        assert!(schema
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required")
            .is_empty());
    }
}
