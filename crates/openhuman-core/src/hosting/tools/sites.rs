//! Site tools: listing sites and setting their environment variables.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyhosts::{DeploymentTarget, EnvVar, Host};

use super::{env_value, required_str};
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ── hosting_list_sites ──────────────────────────────────────────────────────

/// Lists the sites on the account.
pub struct ListSitesTool {
    host: Arc<dyn Host>,
}

impl ListSitesTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for ListSitesTool {
    fn name(&self) -> &str {
        "hosting_list_sites"
    }

    fn description(&self) -> &str {
        "List the sites already on the hosting account, newest first. Use it to \
         find out whether a site exists before deploying, or to recover a name."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
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
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 100) as u32;

        match self.host.list_sites(limit).await {
            Ok(sites) => Ok(ToolResult::success(serde_json::to_string_pretty(&sites)?)),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}

// ── hosting_set_env ─────────────────────────────────────────────────────────

/// Sets environment variables on an existing site.
pub struct SetEnvTool {
    host: Arc<dyn Host>,
}

impl SetEnvTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for SetEnvTool {
    fn name(&self) -> &str {
        "hosting_set_env"
    }

    fn description(&self) -> &str {
        "Set environment variables on a site, replacing any of the same name. \
         The site must be redeployed afterwards for a build-time variable to \
         take effect. Values are write-only: they can never be read back through \
         these tools."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site", "env"],
            "properties": {
                "site": { "type": "string", "description": "The site's name." },
                "env": {
                    "type": "object",
                    "description": "Variables to set.",
                    "additionalProperties": { "type": "string" }
                },
                "secret": {
                    "type": "boolean",
                    "description": "Store them write-only at the provider. Defaults to false."
                },
                "production_only": {
                    "type": "boolean",
                    "description": "Apply to production only rather than every environment."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn external_effect(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let site = match required_str(&args, "site") {
            Ok(site) => site,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };
        let Some(env) = args.get("env").and_then(Value::as_object) else {
            return Ok(ToolResult::error("`env` must be an object of variables"));
        };

        let secret = args.get("secret").and_then(Value::as_bool).unwrap_or(false);
        let targets = if args
            .get("production_only")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            vec![DeploymentTarget::Production]
        } else {
            Vec::new()
        };

        let vars: Vec<EnvVar> = match env
            .iter()
            .map(|(key, value)| {
                let var = EnvVar::new(key, env_value(key, value)?).with_targets(targets.clone());
                Ok(if secret { var.secret() } else { var })
            })
            .collect::<anyhow::Result<Vec<_>>>()
        {
            Ok(vars) => vars,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };

        let names: Vec<&str> = vars.iter().map(|var| var.key.as_str()).collect();
        match self.host.set_env(&site, &vars).await {
            Ok(()) => Ok(ToolResult::success(format!(
                "Set {} on {site}. Redeploy the site for a build-time variable to take effect.",
                names.join(", ")
            ))),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}
