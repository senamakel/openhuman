//! `hosting_analytics`: reports the traffic a site served.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyhosts::{AnalyticsDimension, AnalyticsQuery, Host};

use super::required_str;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ── hosting_analytics ───────────────────────────────────────────────────────

/// Reports the traffic a site served.
pub struct AnalyticsTool {
    host: Arc<dyn Host>,
}

impl AnalyticsTool {
    pub fn new(host: Arc<dyn Host>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl Tool for AnalyticsTool {
    fn name(&self) -> &str {
        "hosting_analytics"
    }

    fn description(&self) -> &str {
        "Report how much traffic a hosted site served over the last N days — \
         visitors and page views, optionally broken down by country, path, \
         device, browser, or referrer."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["site"],
            "properties": {
                "site": { "type": "string", "description": "The site's name." },
                "days": {
                    "type": "integer",
                    "description": "How many days back to report. Defaults to 7."
                },
                "breakdown": {
                    "type": "string",
                    "enum": [
                        "country", "request_path", "device_type",
                        "browser_name", "os_name", "referrer_hostname", "route"
                    ],
                    "description": "Break the totals down by this dimension."
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
        let days = args
            .get("days")
            .and_then(Value::as_u64)
            .unwrap_or(7)
            .clamp(1, 365);

        let until_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let since_ms = until_ms.saturating_sub(days * 24 * 60 * 60 * 1000);

        let mut query = AnalyticsQuery::new(site, since_ms, until_ms);
        if let Some(dimension) = args.get("breakdown").and_then(Value::as_str) {
            let breakdown = match dimension {
                "country" => AnalyticsDimension::Country,
                "request_path" => AnalyticsDimension::RequestPath,
                "device_type" => AnalyticsDimension::DeviceType,
                "browser_name" => AnalyticsDimension::BrowserName,
                "os_name" => AnalyticsDimension::OsName,
                "referrer_hostname" => AnalyticsDimension::ReferrerHostname,
                "route" => AnalyticsDimension::Route,
                other => {
                    return Ok(ToolResult::error(format!(
                        "`breakdown` must be one of country, request_path, device_type, \
                         browser_name, os_name, referrer_hostname, route — not `{other}`"
                    )));
                }
            };
            query = query.with_breakdown(breakdown);
        }

        match self.host.analytics(&query).await {
            Ok(summary) => Ok(ToolResult::success(serde_json::to_string_pretty(&summary)?)),
            Err(error) => Ok(ToolResult::error(error.to_string())),
        }
    }
}
