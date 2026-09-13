//! Deployment tools: status, listing, logs, and rollback.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyhosts::Host;

use super::required_str;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ── hosting_deployment_status ───────────────────────────────────────────────

/// Reads one deployment's current state.
pub struct DeploymentStatusTool {
    host: Arc<dyn Host>,
}

impl DeploymentStatusTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for DeploymentStatusTool {
    fn name(&self) -> &str {
        "hosting_deployment_status"
    }

    fn description(&self) -> &str {
        "Check whether a deployment has finished building and is serving. Poll \
         this after hosting_launch_site until the status is ready, failed, or \
         canceled. A failed deployment reports the provider's build error."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["deployment_id"],
            "properties": {
                "deployment_id": {
                    "type": "string",
                    "description": "The id hosting_launch_site returned."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = match required_str(&args, "deployment_id") {
            Ok(id) => id,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };

        match self.host.deployment(&id).await {
            Ok(deployment) => Ok(ToolResult::success(serde_json::to_string_pretty(
                &deployment,
            )?)),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}

// ── hosting_list_deployments ────────────────────────────────────────────────

/// Lists a site's recent deployments, newest first.
///
/// Mostly the other half of [`RollbackTool`]: a rollback needs a deployment id
/// to promote, and before this tool nothing returned one except the launch that
/// created it. An agent that wanted to go back to the deployment *before* the
/// bad one had no way to name it.
pub struct ListDeploymentsTool {
    host: Arc<dyn Host>,
}

impl ListDeploymentsTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for ListDeploymentsTool {
    fn name(&self) -> &str {
        "hosting_list_deployments"
    }

    fn description(&self) -> &str {
        "List a site's recent deployments, newest first, with their status, \
         target and creation time. Use it to find the deployment id of a known \
         good version before rolling back to it, or to see the history of what \
         has been shipped. Each entry's id is what hosting_rollback and \
         hosting_deployment_status take."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site"],
            "properties": {
                "site": { "type": "string", "description": "The site's name." },
                "limit": {
                    "type": "integer",
                    "description": "How many to return. Defaults to 20."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let site = match required_str(&args, "site") {
            Ok(site) => site,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };
        // Clamped to the same window as `hosting_list_sites`, for the same
        // reason: a model that asks for everything gets a page, not a bill.
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 100) as u32;

        match self.host.list_deployments(&site, limit).await {
            Ok(deployments) => Ok(ToolResult::success(serde_json::to_string_pretty(
                &deployments,
            )?)),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}

// ── hosting_deployment_logs ─────────────────────────────

/// Reads the build and runtime events a deployment recorded.
///
/// The other half of [`DeploymentStatusTool`]. That tool reports *that* a build
/// failed and carries the provider's one-line error; this one is how an agent
/// finds out *why*, which is the difference between reporting a broken deploy
/// and fixing it.
pub struct DeploymentLogsTool {
    host: Arc<dyn Host>,
}

impl DeploymentLogsTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for DeploymentLogsTool {
    fn name(&self) -> &str {
        "hosting_deployment_logs"
    }

    fn description(&self) -> &str {
        "Read a deployment's build and runtime log events, oldest first. Use it \
         after hosting_deployment_status reports a failed deployment to find the \
         error that caused it. Takes the same deployment id as \
         hosting_deployment_status."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["deployment_id"],
            "properties": {
                "deployment_id": {
                    "type": "string",
                    "description": "The id hosting_launch_site returned, or one \
                                    from hosting_list_deployments."
                },
                "limit": {
                    "type": "integer",
                    "description": "How many of the most recent events to return. \
                                    Defaults to 100."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = match required_str(&args, "deployment_id") {
            Ok(id) => id,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };
        let limit = args.get("limit").map_or(100, |value| {
            value
                .as_i64()
                .map(|value| value.clamp(1, 1000) as usize)
                .or_else(|| value.as_u64().map(|value| value.clamp(1, 1000) as usize))
                .unwrap_or(100)
        });

        match self.host.deployment_logs(&id).await {
            Ok(logs) => {
                // The crate returns the whole log oldest-first and a build can
                // record thousands of lines, which is a context window rather
                // than a bill. Trimming takes the *tail*: the failure that sent
                // an agent here is at the end, and dropping the head loses
                // setup noise rather than the error.
                let trimmed = if logs.len() > limit {
                    &logs[logs.len() - limit..]
                } else {
                    &logs[..]
                };

                Ok(ToolResult::success(serde_json::to_string_pretty(&trimmed)?))
            }
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}

// ── hosting_rollback ────────────────────────────────────────────────────────

/// Points a site's production traffic back at an earlier deployment.
pub struct RollbackTool {
    host: Arc<dyn Host>,
}

impl RollbackTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for RollbackTool {
    fn name(&self) -> &str {
        "hosting_rollback"
    }

    fn description(&self) -> &str {
        "Roll a site back by pointing its production traffic at an earlier \
         deployment that already built successfully. Use it when a deploy broke \
         a live site: this is the recovery path. It does not rebuild anything — \
         the deployment being promoted was built when it was first deployed, \
         which is why it is the fast way back. Get the id from \
         hosting_list_deployments; only a deployment that finished building can \
         be promoted."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site", "deployment_id"],
            "properties": {
                "site": {
                    "type": "string",
                    "description": "The site whose production traffic moves."
                },
                "deployment_id": {
                    "type": "string",
                    "description": "The deployment to serve, from hosting_list_deployments. \
                                    It must have finished building."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    /// Changes what the public sees on a live site, so it gates.
    fn external_effect(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let site = match required_str(&args, "site") {
            Ok(site) => site,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };
        let deployment_id = match required_str(&args, "deployment_id") {
            Ok(id) => id,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };

        // Read the deployment before promoting it. `hosting_list_deployments`
        // returns failed and still-building deployments too — they are part of
        // the history an agent is reading — so the id it picks is not
        // necessarily one that can serve traffic. Promoting a failed build
        // would take the site down in the middle of an attempt to bring it
        // back up, which is the one outcome this tool exists to prevent.
        //
        // Only the status is checked, deliberately. The site a deployment
        // belongs to is *not* reliable here: `Host::deployment` looks a
        // deployment up by id alone and falls back to an empty name when the
        // provider's response omits one, so comparing it against `site` would
        // refuse legitimate rollbacks. The provider owns that check.
        let deployment = match self.host.deployment(&deployment_id).await {
            Ok(deployment) => deployment,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };

        if !deployment.status.is_ready() {
            return Ok(ToolResult::error(format!(
                "Deployment `{deployment_id}` is {:?}, so it cannot be promoted \
                 — only a deployment that finished building can serve traffic. \
                 Use hosting_list_deployments to find one that is ready.",
                deployment.status,
            )));
        }

        tracing::info!(
            site = %site,
            deployment = %deployment_id,
            "[hosting] rolling back"
        );

        match self.host.promote(&site, &deployment_id).await {
            Ok(()) => Ok(ToolResult::success(match &deployment.url {
                Some(url) => format!(
                    "{site} is now serving deployment `{deployment_id}` in \
                     production ({url}). The change is at the provider's edge; \
                     nothing was rebuilt."
                ),
                None => format!(
                    "{site} is now serving deployment `{deployment_id}` in \
                     production. The change is at the provider's edge; nothing \
                     was rebuilt."
                ),
            })),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}
