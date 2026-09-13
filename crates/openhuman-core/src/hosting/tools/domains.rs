//! Domain tools: attaching a custom domain and reading its verification.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyhosts::{Domain, Host};

use super::required_str;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ── hosting_add_domain ──────────────────────────────────────────────────────

/// Attaches a custom domain to a site.
pub struct AddDomainTool {
    host: Arc<dyn Host>,
}

impl AddDomainTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for AddDomainTool {
    fn name(&self) -> &str {
        "hosting_add_domain"
    }

    fn description(&self) -> &str {
        "Attach a custom domain to a site. The domain does not serve traffic \
         until its DNS records point at the provider, which the user has to do \
         at their registrar — the response says whether the provider has \
         verified it yet."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site", "domain"],
            "properties": {
                "site": { "type": "string", "description": "The site's name." },
                "domain": { "type": "string", "description": "e.g. 'shop.example.com'." }
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
        let domain = match required_str(&args, "domain") {
            Ok(domain) => domain,
            Err(error) => return Ok(ToolResult::error(error.to_string())),
        };

        match self.host.add_domain(&site, &domain).await {
            Ok(Domain {
                name,
                verified: true,
                ..
            }) => Ok(ToolResult::success(format!(
                "{name} is attached to {site} and verified."
            ))),
            Ok(Domain { name, .. }) => Ok(ToolResult::success(format!(
                "{name} is attached to {site} but not verified yet: its DNS \
                 records still have to point at the provider."
            ))),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}

// ── hosting_domain_status ───────────────────────────────────────────────────

/// Reports whether a site's domains are verified and serving.
///
/// The read half of [`AddDomainTool`]. Attaching a domain is not the end of the
/// job: it does not serve traffic until its DNS records point at the provider,
/// which the user has to do at their registrar, and until now nothing could
/// answer "did that work?" without attaching it again.
pub struct DomainStatusTool {
    host: Arc<dyn Host>,
}

impl DomainStatusTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for DomainStatusTool {
    fn name(&self) -> &str {
        "hosting_domain_status"
    }

    fn description(&self) -> &str {
        "List the custom domains attached to a site and whether the provider \
         has verified each one. A domain that is attached but unverified is not \
         serving traffic yet — its DNS records still have to point at the \
         provider, which the user does at their registrar. Use it to check \
         whether a domain added earlier has come up."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site"],
            "properties": {
                "site": { "type": "string", "description": "The site's name." }
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

        match self.host.list_domains(&site).await {
            Ok(domains) => Ok(ToolResult::success(serde_json::to_string_pretty(&domains)?)),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}
