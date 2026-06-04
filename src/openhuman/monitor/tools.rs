use super::ops;
use super::types::{MonitorReadRequest, MonitorStartRequest, MonitorStopRequest};
use crate::openhuman::agent::host_runtime::RuntimeAdapter;
use crate::openhuman::security::{AuditLogger, GateDecision, SecurityPolicy};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct MonitorTool {
    security: Arc<SecurityPolicy>,
    runtime: Arc<dyn RuntimeAdapter>,
    audit: Arc<AuditLogger>,
}

impl MonitorTool {
    pub fn new(
        security: Arc<SecurityPolicy>,
        runtime: Arc<dyn RuntimeAdapter>,
        audit: Arc<AuditLogger>,
    ) -> Self {
        Self {
            security,
            runtime,
            audit,
        }
    }
}

#[async_trait]
impl Tool for MonitorTool {
    fn name(&self) -> &str {
        "monitor"
    }

    fn description(&self) -> &str {
        "Start a bounded background command monitor. Each stdout/stderr line is stored in workspace monitor output and concise events are collected into the active agent turn when available."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" },
                "description": { "type": "string" },
                "timeout_ms": { "type": "integer", "minimum": 1 },
                "persistent": { "type": "boolean" },
                "category": {
                    "type": "string",
                    "enum": ["read", "write", "network", "install", "destructive"]
                }
            },
            "required": ["command"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    fn external_effect_with_args(&self, args: &serde_json::Value) -> bool {
        let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let mut class = self.security.classify_command(command);
        if let Some(declared) = args
            .get("category")
            .and_then(|v| v.as_str())
            .and_then(SecurityPolicy::parse_declared_class)
        {
            class = class.max(declared);
        }
        self.security.gate_decision(class) == GateDecision::Prompt
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let request: MonitorStartRequest = serde_json::from_value(args)?;
        match ops::start(
            request,
            Arc::clone(&self.security),
            Arc::clone(&self.runtime),
            Arc::clone(&self.audit),
        )
        .await
        {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string(&outcome.value)?)),
            Err(error) => Ok(ToolResult::error(error)),
        }
    }
}

pub struct MonitorListTool;

#[async_trait]
impl Tool for MonitorListTool {
    fn name(&self) -> &str {
        "monitor_list"
    }

    fn description(&self) -> &str {
        "List background command monitors and recent bounded events."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let outcome = ops::list().await.map_err(anyhow::Error::msg)?;
        Ok(ToolResult::success(serde_json::to_string(&outcome.value)?))
    }
}

pub struct MonitorStopTool;

#[async_trait]
impl Tool for MonitorStopTool {
    fn name(&self) -> &str {
        "monitor_stop"
    }

    fn description(&self) -> &str {
        "Stop a running background command monitor."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": { "monitor_id": { "type": "string" } },
            "required": ["monitor_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let request: MonitorStopRequest = serde_json::from_value(args)?;
        match ops::stop(request).await {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string(&outcome.value)?)),
            Err(error) => Ok(ToolResult::error(error)),
        }
    }
}

pub struct MonitorReadTool;

#[async_trait]
impl Tool for MonitorReadTool {
    fn name(&self) -> &str {
        "monitor_read"
    }

    fn description(&self) -> &str {
        "Read bounded output from a background command monitor."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "monitor_id": { "type": "string" },
                "max_bytes": { "type": "integer", "minimum": 1 }
            },
            "required": ["monitor_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let request: MonitorReadRequest = serde_json::from_value(args)?;
        match ops::read(request).await {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string(&outcome.value)?)),
            Err(error) => Ok(ToolResult::error(error)),
        }
    }
}
