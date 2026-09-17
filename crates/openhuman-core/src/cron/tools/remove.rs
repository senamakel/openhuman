use crate::config::Config;
use crate::cron;
use crate::tools::traits::{PermissionLevel, Tool, ToolExposure, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct CronRemoveTool {
    config: Arc<Config>,
}

impl CronRemoveTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CronRemoveTool {
    /// Superseded by the `cron` tool, which dispatches every scheduler
    /// operation on one `action` field. Kept registered and dispatchable so a
    /// replayed transcript or a saved skill naming `cron_*` keeps working;
    /// hidden from the wire so six schemas do not ship where one does.
    fn exposure(&self) -> ToolExposure {
        ToolExposure::Hidden
    }

    fn name(&self) -> &str {
        "cron_remove"
    }

    fn description(&self) -> &str {
        "Remove a cron job by id"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "job_id": { "type": "string" }
            },
            "required": ["job_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn external_effect(&self) -> bool {
        // Removing a job is a persistent mutation; require approval so an
        // inbound-channel message cannot silently cancel scheduled work
        // (GHSA-f46p-6vf9-64mm).
        true
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        if !self.config.cron.enabled {
            return Ok(ToolResult::error(
                "cron is disabled by config (cron.enabled=false)".to_string(),
            ));
        }

        let job_id = match args.get("job_id").and_then(serde_json::Value::as_str) {
            Some(v) if !v.trim().is_empty() => v,
            _ => {
                return Ok(ToolResult::error("Missing 'job_id' parameter".to_string()));
            }
        };

        match cron::remove_job(&self.config, job_id) {
            Ok(()) => Ok(ToolResult::success(format!("Removed cron job {job_id}"))),
            Err(e) => Ok(ToolResult::error(e.to_string())),
        }
    }
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
