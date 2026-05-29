//! Agent-facing tools for the agent_workflows domain.
//!
//! These give the agent the **explicit** half of the Hybrid phase model:
//! - [`WorkflowLoadTool`] (`workflow_load`) pulls a workflow's full definition
//!   into context so the agent can follow it as the task progresses.
//! - [`WorkflowPhaseTool`] (`workflow_phase`) surfaces one phase's rules,
//!   scripts, effective tool scope, and working-directory context.
//!
//! Both are read-only: they surface guidance. The phase's *scripts* are listed
//! for the agent to run through its normal (approval-gated) shell tool — the
//! workflow never executes anything itself, so the `SecurityPolicy` /
//! `ApprovalGate` apply unchanged.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::openhuman::agent_workflows::{
    discover_workflows, effective_tool_scope, is_workspace_trusted, phase_guidance,
    working_dir_context, Workflow,
};
use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};

fn discover_for(config: &Config) -> Vec<Workflow> {
    let home = dirs::home_dir();
    let trusted = is_workspace_trusted(&config.workspace_dir);
    discover_workflows(
        home.as_deref(),
        Some(config.workspace_dir.as_path()),
        trusted,
    )
}

fn find_workflow(config: &Config, id: &str) -> Option<Workflow> {
    discover_for(config)
        .into_iter()
        .find(|w| w.id() == id || w.name == id)
}

/// `workflow_load` — pull a workflow's full definition (all phases) into context.
pub struct WorkflowLoadTool {
    config: Arc<Config>,
}

impl WorkflowLoadTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowLoadTool {
    fn name(&self) -> &str {
        "workflow_load"
    }

    fn description(&self) -> &str {
        "Load a workflow's full definition (its phases: rules, scripts, tool scope, working-dir \
         context) by id. Call this when the current task matches a workflow's `when_to_use`; its \
         phase guidance then steers how you approach the task. Read-only — it surfaces guidance, \
         it does not run anything."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "description": "The workflow id (on-disk slug), as listed in the Available Workflows catalog."
                }
            },
            "required": ["workflow_id"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let id = args
            .get("workflow_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if id.is_empty() {
            return Ok(ToolResult::error("workflow_id is required"));
        }
        tracing::debug!(workflow_id = %id, "[workflows][tool] workflow_load");
        let Some(workflow) = find_workflow(&self.config, id) else {
            return Ok(ToolResult::error(format!("workflow '{id}' not found")));
        };
        Ok(ToolResult::success(render_workflow_markdown(&workflow)))
    }
}

/// `workflow_phase` — surface one phase's guidance + context for a workflow.
pub struct WorkflowPhaseTool {
    config: Arc<Config>,
}

impl WorkflowPhaseTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for WorkflowPhaseTool {
    fn name(&self) -> &str {
        "workflow_phase"
    }

    fn description(&self) -> &str {
        "Enter a phase of an active workflow and get its rules, the scripts to run (run them via \
         your shell tool — they go through the normal approval gate), the tools you should limit \
         yourself to, and a working-directory snapshot (e.g. git branch/status). Phases: \
         on_pick_up_task, on_close_task, on_enter_directory, or a custom name."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "description": "The workflow id (on-disk slug)."
                },
                "phase": {
                    "type": "string",
                    "description": "Phase name, e.g. on_pick_up_task / on_close_task / on_enter_directory."
                }
            },
            "required": ["workflow_id", "phase"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let id = args
            .get("workflow_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        let phase = args
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if id.is_empty() || phase.is_empty() {
            return Ok(ToolResult::error("workflow_id and phase are required"));
        }
        tracing::debug!(workflow_id = %id, phase = %phase, "[workflows][tool] workflow_phase");
        let Some(workflow) = find_workflow(&self.config, id) else {
            return Ok(ToolResult::error(format!("workflow '{id}' not found")));
        };
        let Some(phase_def) = workflow.phase(phase).cloned() else {
            return Ok(ToolResult::error(format!(
                "workflow '{id}' has no phase '{phase}'"
            )));
        };

        let mut out = String::new();
        if let Some(guidance) = phase_guidance(&workflow, phase) {
            out.push_str(&guidance);
        } else {
            out.push_str(&format!(
                "## Workflow: {} — phase `{phase}`\n(no rules or scripts for this phase)\n",
                workflow.name
            ));
        }

        if let Some(scope) = effective_tool_scope(&workflow, phase) {
            out.push_str("\n**Limit yourself to these tools for this phase:**\n");
            if !scope.allow.is_empty() {
                out.push_str(&format!("- allow: {}\n", scope.allow.join(", ")));
            }
            if !scope.deny.is_empty() {
                out.push_str(&format!("- avoid: {}\n", scope.deny.join(", ")));
            }
        }

        if !phase_def.context.is_empty() {
            if let Some(block) =
                working_dir_context(self.config.workspace_dir.as_path(), &phase_def.context).await
            {
                out.push('\n');
                out.push_str(&block);
            }
        }

        Ok(ToolResult::success(out))
    }
}

fn render_workflow_markdown(workflow: &Workflow) -> String {
    use std::fmt::Write as _;
    let mut out = format!(
        "# Workflow: {}\n\n{}\n",
        workflow.name, workflow.description
    );
    if let Some(w) = workflow.when_to_use.as_deref() {
        let _ = writeln!(out, "\n_When to use:_ {w}");
    }
    if let Some(scope) = workflow.tools.as_ref().filter(|s| !s.is_empty()) {
        let _ = writeln!(
            out,
            "\n_Default tool scope:_ allow [{}]",
            scope.allow.join(", ")
        );
    }
    if workflow.phases.is_empty() {
        out.push_str("\n_This workflow declares no phases._\n");
        return out;
    }
    out.push_str("\n## Phases\n");
    for (name, phase) in &workflow.phases {
        let _ = writeln!(out, "\n### `{name}`");
        if let Some(d) = phase.description.as_deref() {
            let _ = writeln!(out, "{d}");
        }
        for rule in &phase.rules {
            let _ = writeln!(out, "- rule: {}", rule.trim());
        }
        for script in &phase.scripts {
            let _ = writeln!(
                out,
                "- script (run via your shell tool): `{}`",
                script.trim()
            );
        }
        if !phase.context.is_empty() {
            let _ = writeln!(out, "- context: {}", phase.context.join(", "));
        }
    }
    out.push_str(
        "\nAs you reach each phase of the task, call `workflow_phase` with the phase name to get \
         its rules, run its scripts, and see the working-directory context.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const SAMPLE: &str = r#"---
name: demo
description: Demo workflow
when_to_use: testing the workflow tools
phases:
  on_pick_up_task:
    rules:
      - Plan before acting.
    scripts:
      - echo hello
  on_close_task:
    rules:
      - Verify before closing.
---
# demo
"#;

    fn config_with_workflow(tmp: &TempDir) -> Arc<Config> {
        // Use a trusted project-scope workflow so discovery resolves it purely
        // from `workspace_dir` — no HOME env mutation (which is unsafe + racy
        // across parallel tests).
        let ws = tmp.path().join("ws");
        let dir = ws.join(".openhuman").join("workflows").join("demo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("WORKFLOW.md"), SAMPLE).unwrap();
        fs::write(ws.join(".openhuman").join("trust"), "").unwrap();
        Arc::new(Config {
            workspace_dir: ws,
            ..Config::default()
        })
    }

    #[tokio::test]
    async fn workflow_load_returns_phases() {
        let tmp = TempDir::new().unwrap();
        let cfg = config_with_workflow(&tmp);
        let tool = WorkflowLoadTool::new(cfg);
        let res = tool
            .execute(json!({ "workflow_id": "demo" }))
            .await
            .unwrap();
        assert!(!res.is_error);
        let out = res.output();
        assert!(out.contains("Workflow: demo"));
        assert!(out.contains("on_pick_up_task"));
        assert!(out.contains("echo hello"));
    }

    #[tokio::test]
    async fn workflow_load_unknown_id_errors() {
        let tmp = TempDir::new().unwrap();
        let cfg = config_with_workflow(&tmp);
        let tool = WorkflowLoadTool::new(cfg);
        let res = tool
            .execute(json!({ "workflow_id": "nope" }))
            .await
            .unwrap();
        assert!(res.is_error);
    }

    #[tokio::test]
    async fn workflow_phase_returns_rules_and_scripts() {
        let tmp = TempDir::new().unwrap();
        let cfg = config_with_workflow(&tmp);
        let tool = WorkflowPhaseTool::new(cfg);
        let res = tool
            .execute(json!({ "workflow_id": "demo", "phase": "on_pick_up_task" }))
            .await
            .unwrap();
        assert!(!res.is_error);
        let out = res.output();
        assert!(out.contains("Plan before acting."));
        assert!(out.contains("echo hello"));
    }

    #[tokio::test]
    async fn workflow_phase_unknown_phase_errors() {
        let tmp = TempDir::new().unwrap();
        let cfg = config_with_workflow(&tmp);
        let tool = WorkflowPhaseTool::new(cfg);
        let res = tool
            .execute(json!({ "workflow_id": "demo", "phase": "on_nope" }))
            .await
            .unwrap();
        assert!(res.is_error);
    }
}
