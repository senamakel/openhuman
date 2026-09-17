//! The opt-in, destructive workflow uninstall tool.

use async_trait::async_trait;
use serde_json::json;

use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

use super::helpers::read_required_str;
use crate::skills::ops_install::{uninstall_workflow, UninstallWorkflowParams};

/// Uninstall a user skill. **Deletes from disk** — default-OFF.
pub struct WorkflowUninstallTool;

#[async_trait]
impl Tool for WorkflowUninstallTool {
    fn name(&self) -> &str {
        "uninstall_workflow"
    }

    fn description(&self) -> &str {
        "Uninstall a user-scope workflow by `name`, deleting its directory under \
         `~/.openhuman/skills/`. Irreversible; project/legacy workflows are \
         read-only and cannot be removed. Only use when the user asks to remove \
         a specific workflow."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "name": { "type": "string", "description": "Workflow name (directory) to remove." } },
            "required": ["name"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Dangerous
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][skills] uninstall invoked");
        let name = read_required_str(&args, "name")?;
        let outcome = uninstall_workflow(UninstallWorkflowParams { name }, None)
            .map_err(|e| anyhow::anyhow!("uninstall_workflow: {e}"))?;
        Ok(ToolResult::success(serde_json::to_string(&outcome)?))
    }
}
