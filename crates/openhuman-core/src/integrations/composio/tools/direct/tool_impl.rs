//! The generic-agent-facing `Tool` trait implementation for `ComposioTool`
//! (`list` / `execute` / `connect` dispatch).

use super::types::ComposioTool;
use crate::security::policy::ToolOperation;
use crate::tools::traits::{Tool, ToolCategory, ToolResult};
use async_trait::async_trait;
use serde_json::json;

#[async_trait]
impl Tool for ComposioTool {
    fn name(&self) -> &str {
        "composio"
    }

    fn description(&self) -> &str {
        "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). \
         Use action='list' to see available actions, action='execute' with action_name/tool_slug, params, and optional connected_account_id, \
         or action='connect' with app/auth_config_id to get OAuth URL. \
         For Gmail: GMAIL_FETCH_EMAILS supports standard Gmail search syntax in the 'query' param — \
         use query='from:me' or query='label:SENT' to retrieve sent emails, query='label:INBOX' for inbox, \
         query='is:unread' for unread mail, etc. Sent mail is synced and searchable."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "The operation: 'list' (list available actions), 'execute' (run an action), or 'connect' (get OAuth URL)",
                    "enum": ["list", "execute", "connect"]
                },
                "app": {
                    "type": "string",
                    "description": "Toolkit slug filter for 'list', or toolkit/app for 'connect' (e.g. 'gmail', 'notion', 'github')"
                },
                "action_name": {
                    "type": "string",
                    "description": "Action/tool identifier to execute (legacy aliases supported)"
                },
                "tool_slug": {
                    "type": "string",
                    "description": "Preferred v3 tool slug to execute (alias of action_name)"
                },
                "params": {
                    "type": "object",
                    "description": "Parameters to pass to the action"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity/user ID for multi-user setups (defaults to composio.entity_id from config)"
                },
                "auth_config_id": {
                    "type": "string",
                    "description": "Optional Composio v3 auth config id for connect flow"
                },
                "connected_account_id": {
                    "type": "string",
                    "description": "Optional connected account ID for execute flow when a specific account is required"
                }
            },
            "required": ["action"]
        })
    }

    fn category(&self) -> ToolCategory {
        // Composio proxies to external SaaS (Gmail, Notion, …) — surface
        // it in the Workflow category so the skills sub-agent
        // (`category_filter = "skill"`) can see and call it.
        ToolCategory::Workflow
    }

    fn external_effect(&self) -> bool {
        // Conservative default for the arg-less path: assume any
        // composio call is a write so callers that don't reach the
        // args-aware override still get gated. The harness uses
        // `external_effect_with_args` (below) which inspects
        // `action` and lets read-only branches through.
        true
    }

    fn external_effect_with_args(&self, args: &serde_json::Value) -> bool {
        // `action="list"` enumerates available Composio actions —
        // a read-only catalog call. `action="connect"` only returns
        // an OAuth URL the user then visits manually; the
        // subsequent OAuth handoff is its own consent flow so the
        // tool call itself has no outbound side effect to gate.
        // `action="execute"` (or anything unknown / missing) is the
        // write path and routes through the approval gate.
        !matches!(
            args.get("action").and_then(|v| v.as_str()),
            Some("list") | Some("connect")
        )
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

        let entity_id = args
            .get("entity_id")
            .and_then(|v| v.as_str())
            .unwrap_or(self.default_entity_id.as_str());

        match action {
            "list" => {
                let app = args.get("app").and_then(|v| v.as_str());
                match self.list_actions(app).await {
                    Ok(actions) => {
                        let summary: Vec<String> = actions
                            .iter()
                            .take(20)
                            .map(|a| {
                                format!(
                                    "- {} ({}): {}",
                                    a.name,
                                    a.app_name.as_deref().unwrap_or("?"),
                                    a.description.as_deref().unwrap_or("")
                                )
                            })
                            .collect();
                        let total = actions.len();
                        let output = format!(
                            "Found {total} available actions:\n{}{}",
                            summary.join("\n"),
                            if total > 20 {
                                format!("\n... and {} more", total - 20)
                            } else {
                                String::new()
                            }
                        );
                        Ok(ToolResult::success(output))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Failed to list actions: {e}"))),
                }
            }

            "execute" => {
                if let Err(error) = self
                    .security
                    .enforce_tool_operation(ToolOperation::Act, "composio.execute")
                {
                    return Ok(ToolResult::error(error));
                }

                let action_name = args
                    .get("tool_slug")
                    .or_else(|| args.get("action_name"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Missing 'action_name' (or 'tool_slug') for execute")
                    })?;

                let params = args.get("params").cloned().unwrap_or(json!({}));
                let acct_ref = args.get("connected_account_id").and_then(|v| v.as_str());

                match self
                    .execute_action(action_name, params, Some(entity_id), acct_ref)
                    .await
                {
                    Ok(result) => {
                        let output = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| format!("{result:?}"));
                        Ok(ToolResult::success(output))
                    }
                    Err(e) => Ok(ToolResult::error(format!("Action execution failed: {e}"))),
                }
            }

            "connect" => {
                if let Err(error) = self
                    .security
                    .enforce_tool_operation(ToolOperation::Act, "composio.connect")
                {
                    return Ok(ToolResult::error(error));
                }

                let app = args.get("app").and_then(|v| v.as_str());
                let auth_config_id = args.get("auth_config_id").and_then(|v| v.as_str());

                if app.is_none() && auth_config_id.is_none() {
                    anyhow::bail!("Missing 'app' or 'auth_config_id' for connect");
                }

                match self
                    .get_connection_url(app, auth_config_id, entity_id)
                    .await
                {
                    Ok(url) => {
                        let target =
                            app.unwrap_or(auth_config_id.unwrap_or("provided auth config"));
                        Ok(ToolResult::success(format!(
                            "Open this URL to connect {target}:\n{url}"
                        )))
                    }
                    Err(e) => Ok(ToolResult::error(format!(
                        "Failed to get connection URL: {e}"
                    ))),
                }
            }

            _ => Ok(ToolResult::error(format!(
                "Unknown action '{action}'. Use 'list', 'execute', or 'connect'."
            ))),
        }
    }
}
