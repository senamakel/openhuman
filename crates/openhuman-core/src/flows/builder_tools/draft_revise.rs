//! `revise_workflow`: validate a revised draft and emit a proposal (never persists).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::Config;
use crate::flows::ops;
use crate::flows::ops::validate_and_migrate_graph;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ─────────────────────────────────────────────────────────────────────────────
// revise_workflow — iterative refine of an existing draft (proposal only)
// ─────────────────────────────────────────────────────────────────────────────

/// `revise_workflow`: validate a **revised** draft graph and return the same
/// `workflow_proposal` payload as `propose_workflow`.
///
/// Framed for iterative refinement: the agent supplies the updated `graph` (its
/// revision of a prior draft) plus the `instruction` that motivated the change;
/// the tool validates via the exact same [`validate_and_migrate_graph`] path
/// `flows_create` uses and echoes an optional `revision` note. It NEVER
/// persists — identical human-in-the-loop invariant to
/// [`crate::flows::tools::ProposeWorkflowTool`].
pub struct ReviseWorkflowTool {
    config: Arc<Config>,
}

impl ReviseWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ReviseWorkflowTool {
    fn name(&self) -> &str {
        "revise_workflow"
    }

    fn description(&self) -> &str {
        "Refine an EXISTING workflow draft: supply the full updated tinyflows \
         WorkflowGraph (your revision applied to the prior draft — NOT a \
         regeneration from scratch) plus the `instruction` that motivated the \
         change. Like propose_workflow, this ONLY VALIDATES the revised graph \
         and returns a proposal summary for the user to review — it NEVER \
         creates, updates, or enables the flow. Same graph shape and node kinds \
         as propose_workflow. If validation fails, fix the graph and call again."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable name for the (revised) proposed flow."
                },
                "graph": {
                    "type": "object",
                    "description": "The full REVISED tinyflows WorkflowGraph: { name?, nodes: [...], edges: [...] }. Apply your changes to the prior draft and pass the whole graph — see propose_workflow for node kinds and config shapes.",
                    "properties": {
                        "nodes": { "type": "array" },
                        "edges": { "type": "array" }
                    },
                    "required": ["nodes", "edges"]
                },
                "instruction": {
                    "type": "string",
                    "description": "The revision instruction that motivated this change (e.g. 'add a Slack step after the summary'). Echoed back for the review card; does not affect validation."
                },
                "require_approval": {
                    "type": "boolean",
                    "description": "Force a human-approval gate on every outbound action once saved. Defaults to true for agent-proposed flows."
                }
            },
            "required": ["name", "graph"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Pure validation, no side effect — mirrors propose_workflow.
        PermissionLevel::None
    }

    fn external_effect(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let name = match args.get("name").and_then(Value::as_str).map(str::trim) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => return Ok(ToolResult::error("Missing 'name' parameter".to_string())),
        };
        let graph_json = match args.get("graph") {
            Some(v) if !v.is_null() => v.clone(),
            _ => return Ok(ToolResult::error("Missing 'graph' parameter".to_string())),
        };
        let instruction = args
            .get("instruction")
            .and_then(Value::as_str)
            .map(str::to_string);
        let require_approval = args
            .get("require_approval")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        tracing::debug!(
            target: "flows",
            %name,
            require_approval,
            has_instruction = instruction.is_some(),
            workspace = %self.config.workspace_dir.display(),
            "[flows] revise_workflow: validating revised candidate graph"
        );

        let graph = match validate_and_migrate_graph(graph_json) {
            Ok(graph) => graph,
            Err(e) => {
                tracing::debug!(target: "flows", %name, error = %e, "[flows] revise_workflow: validation failed");
                return Ok(ToolResult::error(format!(
                    "Revised workflow graph is invalid: {e}. Fix the graph and call \
                     revise_workflow again."
                )));
            }
        };

        // Full builder hard-gate stack (binding-resolvability → tool-contract →
        // required-arg resolvability) + summary/warning assembly, shared with
        // edit_workflow so the two proposal paths can't drift.
        match ops::build_builder_proposal(
            &self.config,
            "revise_workflow",
            &name,
            &graph,
            require_approval,
            true,
            instruction,
            // revise_workflow takes only an inline graph — no draft/flow handle
            // to echo. The payload still carries persisted:false unconditionally.
            None,
            None,
        )
        .await
        {
            Ok(payload) => Ok(ToolResult::success(serde_json::to_string_pretty(&payload)?)),
            Err(message) => {
                tracing::debug!(target: "flows", %name, "[flows] revise_workflow: a hard gate rejected the revised graph");
                Ok(ToolResult::error(message))
            }
        }
    }
}
