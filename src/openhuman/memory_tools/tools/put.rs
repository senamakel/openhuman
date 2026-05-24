//! `memory_tools_put` — upsert a tool-scoped memory rule.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::openhuman::memory::ops::helpers::active_memory_client;
use crate::openhuman::memory_tools::{
    ToolMemoryPriority, ToolMemoryRule, ToolMemorySource, ToolMemoryStore,
};
use crate::openhuman::tools::traits::{Tool, ToolResult};

pub struct MemoryToolsPutTool;

#[derive(Debug, Deserialize)]
struct Args {
    tool_name: String,
    rule: String,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

fn parse_priority(s: Option<&str>) -> ToolMemoryPriority {
    match s.map(|x| x.to_ascii_lowercase()) {
        Some(ref v) if v == "critical" => ToolMemoryPriority::Critical,
        Some(ref v) if v == "high" => ToolMemoryPriority::High,
        _ => ToolMemoryPriority::Normal,
    }
}

#[async_trait]
impl Tool for MemoryToolsPutTool {
    fn name(&self) -> &str {
        "memory_tools_put"
    }

    fn description(&self) -> &str {
        "Record a durable rule / learning for the given tool. Use when the \
         user gives a directive that should survive future sessions, or \
         when a tool failure pattern is worth pinning. Returns the stored \
         rule with its assigned id and timestamps."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["tool_name", "rule"],
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Exact tool name the rule applies to."
                },
                "rule": {
                    "type": "string",
                    "description": "Free-text rule, edict, or learning to pin."
                },
                "priority": {
                    "type": "string",
                    "enum": ["critical", "high", "normal"],
                    "description": "How aggressively to surface the rule. Default: normal."
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional free-form tags (e.g. `safety`, `permission`)."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let parsed: Args = serde_json::from_value(args)
            .map_err(|e| anyhow::anyhow!("invalid arguments for memory_tools_put: {e}"))?;
        log::debug!(
            "[tool][memory_tools] put tool_name={} priority={:?} tags={}",
            parsed.tool_name,
            parsed.priority,
            parsed.tags.len()
        );
        let client = active_memory_client()
            .await
            .map_err(|e| anyhow::anyhow!("memory_tools_put: {e}"))?;
        let store = ToolMemoryStore::new(client.memory_handle());
        let mut rule = ToolMemoryRule::new(
            &parsed.tool_name,
            &parsed.rule,
            parse_priority(parsed.priority.as_deref()),
            ToolMemorySource::UserExplicit,
        );
        rule.tags = parsed.tags;
        let stored = store
            .put_rule(rule)
            .await
            .map_err(|e| anyhow::anyhow!("memory_tools_put: {e}"))?;
        let json = serde_json::to_string(&stored)?;
        Ok(ToolResult::success(json))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_priority_defaults_to_normal() {
        assert_eq!(parse_priority(None), ToolMemoryPriority::Normal);
        assert_eq!(parse_priority(Some("normal")), ToolMemoryPriority::Normal);
        assert_eq!(parse_priority(Some("unknown")), ToolMemoryPriority::Normal);
    }

    #[test]
    fn parse_priority_accepts_critical_and_high_case_insensitively() {
        assert_eq!(parse_priority(Some("critical")), ToolMemoryPriority::Critical);
        assert_eq!(parse_priority(Some("CRITICAL")), ToolMemoryPriority::Critical);
        assert_eq!(parse_priority(Some("high")), ToolMemoryPriority::High);
        assert_eq!(parse_priority(Some("HiGh")), ToolMemoryPriority::High);
    }

    #[test]
    fn args_default_tags_to_empty() {
        let args: Args = serde_json::from_value(json!({
            "tool_name": "bash",
            "rule": "Never run rm -rf"
        }))
        .unwrap();
        assert_eq!(args.tool_name, "bash");
        assert_eq!(args.rule, "Never run rm -rf");
        assert!(args.priority.is_none());
        assert!(args.tags.is_empty());
    }

    #[test]
    fn parameters_schema_describes_priority_enum() {
        let tool = MemoryToolsPutTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], json!(["tool_name", "rule"]));
        assert_eq!(
            schema["properties"]["priority"]["enum"],
            json!(["critical", "high", "normal"])
        );
    }
}
