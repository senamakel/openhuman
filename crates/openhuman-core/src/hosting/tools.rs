//! The `hosting_*` agent tools.
//!
//! Ten tools over one hosting account. They are thin on purpose: argument
//! parsing, one call into [`tinyhosts`], and a result described for a model.
//! Anything that looks like hosting logic belongs in the crate, where it is
//! provider-independent and tested against a mock of the provider's API.
//!
//! `hosting_launch_site` uploads a directory to a third party and can spend
//! money on a database, and `hosting_rollback` repoints a live site's
//! production traffic; both route through the approval gate, as do
//! `hosting_set_env` and `hosting_add_domain`. The rest read.
//!
//! # Why there is a rollback but no separate "promote"
//!
//! [`Host::promote`] is both: a rollback *is* a promote of an older deployment,
//! and the crate models it once deliberately. The tool is named for the reason
//! an agent reaches for it. Without it an agent can deploy a broken site and
//! have no way back, which is the whole argument for the tool existing.

mod analytics;
mod deployments;
mod domains;
mod launch;
mod sites;

use serde_json::Value;

pub use analytics::AnalyticsTool;
pub use deployments::{
    DeploymentLogsTool, DeploymentStatusTool, ListDeploymentsTool, RollbackTool,
};
pub use domains::{AddDomainTool, DomainStatusTool};
pub use launch::LaunchSiteTool;
pub use sites::{ListSitesTool, SetEnvTool};

use super::Account;
use crate::tools::traits::Tool;

/// Every hosting tool, for one account.
pub fn hosting_tools(account: &Account) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(LaunchSiteTool::new(account.clone())),
        Box::new(DeploymentStatusTool::new(account.host())),
        Box::new(ListDeploymentsTool::new(account.host())),
        Box::new(DeploymentLogsTool::new(account.host())),
        Box::new(RollbackTool::new(account.host())),
        Box::new(ListSitesTool::new(account.host())),
        Box::new(SetEnvTool::new(account.host())),
        Box::new(AddDomainTool::new(account.host())),
        Box::new(DomainStatusTool::new(account.host())),
        Box::new(AnalyticsTool::new(account.host())),
    ]
}

/// Reads a required string argument.
fn required_str(args: &Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("`{key}` is required"))
}

/// Renders one `env` object value. A number or a bool is still a variable, so
/// it is rendered rather than dropped. `null` and a container are refused: a
/// variable silently set to `"null"` is worse than a named error.
fn env_value(key: &str, value: &Value) -> anyhow::Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        _ => anyhow::bail!("`env.{key}` must be a string, number, or boolean"),
    }
}
