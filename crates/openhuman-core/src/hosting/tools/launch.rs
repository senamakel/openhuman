//! `hosting_launch_site`: deploys a workspace directory as a live site.

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyhosts::{Bundle, DatabaseKind, DatabaseSpec, EnvVar, Launch, LaunchPlan, SiteSpec};

use super::{env_value, required_str};
use crate::hosting::{resolve_in_workspace, Account};
use crate::tools::traits::{PermissionLevel, Tool, ToolCallOptions, ToolResult};

/// Renders a launch as the two sentences a model needs: where it is, and what
/// it still has to wait for.
fn describe(launch: &Launch) -> String {
    let mut lines = vec![format!(
        "Site **{}** ({}), deployment `{}` is {:?}.",
        launch.site.name,
        if launch.created_site {
            "created"
        } else {
            "already existed"
        },
        launch.deployment.id,
        launch.deployment.status,
    )];

    match launch.url() {
        Some(url) => lines.push(format!(
            "It will serve from {url} once the build finishes — poll \
             `hosting_deployment_status` with the deployment id."
        )),
        None => lines.push(
            "The provider has not assigned a URL yet; poll \
             `hosting_deployment_status` with the deployment id."
                .to_string(),
        ),
    }

    if let Some(database) = &launch.database {
        lines.push(format!(
            "Database **{}** ({}) is {}; it injected {} into the site's \
             environment. The values are the provider's — nothing here can read them.",
            database.name,
            database.kind.as_str(),
            database.status,
            if launch.database_env_keys.is_empty() {
                "no variables".to_string()
            } else {
                launch.database_env_keys.join(", ")
            },
        ));
    }

    if !launch.domains.is_empty() {
        let unverified: Vec<&str> = launch
            .domains
            .iter()
            .filter(|domain| !domain.verified)
            .map(|domain| domain.name.as_str())
            .collect();
        if unverified.is_empty() {
            lines.push("Every domain is verified.".to_string());
        } else {
            lines.push(format!(
                "These domains still need their DNS records pointed at the \
                 provider before they serve traffic: {}.",
                unverified.join(", ")
            ));
        }
    }

    lines.join("\n\n")
}

// ── hosting_launch_site ─────────────────────────────────────────────────────

/// Deploys a workspace directory as a live site, with an optional database.
pub struct LaunchSiteTool {
    account: Account,
}

impl LaunchSiteTool {
    pub fn new(account: Account) -> Self {
        Self { account }
    }

    /// Builds the plan an invocation describes.
    fn plan(&self, args: &Value) -> anyhow::Result<LaunchPlan> {
        let site = required_str(args, "site")?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .to_string();

        let directory = resolve_in_workspace(self.account.workspace_dir(), &path)?;
        let bundle = Bundle::from_dir(&directory)?;

        let mut plan = LaunchPlan::new(SiteSpec::new(site), bundle);

        if let Some(name) = args.get("database").and_then(Value::as_str) {
            let name = name.trim();
            if !name.is_empty() {
                let kind = match args
                    .get("database_kind")
                    .and_then(Value::as_str)
                    .unwrap_or("postgres")
                {
                    "postgres" => DatabaseKind::Postgres,
                    "redis" => DatabaseKind::Redis,
                    "blob" => DatabaseKind::Blob,
                    other => DatabaseKind::Other(other.to_string()),
                };
                plan = plan.with_database(DatabaseSpec::new(name).with_kind(kind));
            }
        }

        if let Some(env) = args.get("env").and_then(Value::as_object) {
            let vars = env
                .iter()
                .map(|(key, value)| Ok(EnvVar::new(key, env_value(key, value)?)))
                .collect::<anyhow::Result<Vec<_>>>()?;
            plan = plan.with_env(vars);
        }

        if let Some(domains) = args.get("domains").and_then(Value::as_array) {
            plan = plan.with_domains(
                domains
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|domain| !domain.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
            );
        }

        if args
            .get("production")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            plan = plan.into_production();
        }

        Ok(plan)
    }
}

#[async_trait]
impl Tool for LaunchSiteTool {
    fn name(&self) -> &str {
        "hosting_launch_site"
    }

    fn description(&self) -> &str {
        "Deploy a directory in the workspace to a real hosting provider as a \
         live website, optionally provisioning a managed database and wiring it \
         in. Creates the site if it does not exist yet, so calling it again \
         redeploys. Use for a Next.js application or a static site. The build \
         starts immediately and finishes later: poll hosting_deployment_status \
         with the returned deployment id until it is ready. Node dependencies, \
         build output and .git are never uploaded — the provider builds from \
         source."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site"],
            "properties": {
                "site": {
                    "type": "string",
                    "description": "The site's name on the provider, e.g. 'acme-shop'. \
                                    Reused on a redeploy."
                },
                "path": {
                    "type": "string",
                    "description": "Directory to deploy, relative to the workspace. \
                                    Defaults to the workspace root."
                },
                "database": {
                    "type": "string",
                    "description": "Name for a managed database to provision and connect. \
                                    Omit if the site needs none. The connection variables \
                                    are injected by the provider before the build."
                },
                "database_kind": {
                    "type": "string",
                    "enum": ["postgres", "redis", "blob"],
                    "description": "What the database speaks. Defaults to postgres."
                },
                "env": {
                    "type": "object",
                    "description": "Environment variables to set before the build. \
                                    Do not put a database connection string here; the \
                                    provider injects its own.",
                    "additionalProperties": { "type": "string" }
                },
                "domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Custom domains to attach. They need DNS records \
                                    pointed at the provider before they serve traffic."
                },
                "production": {
                    "type": "boolean",
                    "description": "Deploy to production rather than to a preview URL. \
                                    Defaults to false."
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

    fn supports_markdown(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    async fn execute_with_options(
        &self,
        args: Value,
        _options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        let plan = match self.plan(&args) {
            Ok(plan) => plan,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };

        tracing::info!(
            site = %plan.site.name,
            files = plan.bundle.len(),
            bytes = plan.bundle.total_bytes(),
            database = plan.database.is_some(),
            target = plan.target.as_str(),
            "[hosting] launching"
        );

        match tinyhosts::launch(self.account.host().as_ref(), &plan).await {
            Ok(launch) => Ok(ToolResult::success_with_markdown(
                serde_json::to_value(&launch)?,
                describe(&launch),
            )),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}
