//! `validate_workflow`: run the full gate stack on a draft without proposing (F3).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::Config;
use crate::flows::ops;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ─────────────────────────────────────────────────────────────────────────────
// validate_workflow — standalone check without proposing (F3)
// ─────────────────────────────────────────────────────────────────────────────

/// `validate_workflow`: run the SAME structural validation + hard-gate stack
/// the propose/revise/edit/save tools use, but WITHOUT emitting a proposal —
/// a pure check so the agent can verify a draft (or a saved flow) mid-build.
///
/// Returns a structured report `{ ok, structurally_valid, errors[],
/// error_details[], gate_errors[], warnings[] }`, so a failing check is
/// fix-and-retry rather than a proposal the user has to reject.
pub struct ValidateWorkflowTool {
    config: Arc<Config>,
}

impl ValidateWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ValidateWorkflowTool {
    fn name(&self) -> &str {
        "validate_workflow"
    }

    fn description(&self) -> &str {
        "Check a workflow graph WITHOUT proposing or saving it — the same validation the \
         propose/revise/edit/save tools run, surfaced on its own so you can verify a draft mid-\
         build. Provide the graph to check as exactly one of `draft_id` (a working draft), \
         `flow_id` (a saved flow), or inline `graph` (if several are given, draft_id wins, then \
         flow_id). Returns { ok, structurally_valid, errors, error_details:[{code, message, \
         node_id}], gate_errors, warnings }: `errors` lists EVERY structural problem at once; \
         `gate_errors` lists the hard author-gate failures (unresolvable bindings, unreal tool \
         slugs, unwired required args) checked only once the graph is structurally valid; \
         `warnings` are non-fatal. `ok` is true only when there are no errors and no gate_errors. \
         Read-only."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "A working draft to validate. Provide one of draft_id / flow_id / graph (draft_id wins)."
                },
                "flow_id": {
                    "type": "string",
                    "description": "A saved flow to validate. Provide one of draft_id / flow_id / graph."
                },
                "graph": {
                    "type": "object",
                    "description": "An inline tinyflows WorkflowGraph to validate. Provide one of draft_id / flow_id / graph.",
                    "properties": {
                        "nodes": { "type": "array" },
                        "edges": { "type": "array" }
                    }
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::None
    }

    fn external_effect(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        // Resolve the graph to check from exactly one of a working draft, a
        // saved flow, or an inline graph — same precedence (draft_id > flow_id >
        // graph) as edit_workflow, so the sibling tools accept the same handles.
        let draft_id = args
            .get("draft_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let flow_id = args
            .get("flow_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let inline_graph = args.get("graph").filter(|v| !v.is_null());

        let graph_json = match (draft_id, flow_id, inline_graph) {
            (Some(id), _, _) => match ops::flows_draft_get(&self.config, id) {
                Ok(outcome) => outcome.value.graph,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "Could not load draft '{id}' to validate: {e}"
                    )));
                }
            },
            (None, Some(id), _) => match ops::load_flow_graph(&self.config, id) {
                Ok(Some(graph)) => serde_json::to_value(&graph)?,
                Ok(None) => {
                    return Ok(ToolResult::error(format!("flow '{id}' not found")));
                }
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "Could not load flow '{id}' to validate: {e}"
                    )));
                }
            },
            (None, None, Some(graph)) => graph.clone(),
            (None, None, None) => {
                return Ok(ToolResult::error(
                    "Provide one of `draft_id` (a working draft), `flow_id` (a saved flow), or \
                     `graph` (an inline graph) to validate."
                        .to_string(),
                ));
            }
        };

        tracing::debug!(
            target: "flows",
            from_draft = draft_id.is_some(),
            from_flow = flow_id.is_some(),
            "[flows] validate_workflow: checking graph (read-only)"
        );

        // Structural validation first (every error at once).
        let validation = ops::flows_validate(graph_json.clone()).value;

        // Only run the (expensive) hard gates on a structurally-valid graph.
        // A migrate/deserialize error here must fail CLOSED: `validation.valid`
        // only proves the graph passed structural checks, not that the hard
        // gates (unresolvable bindings, unreal tool slugs, unwired required
        // args) ran. Treating the empty `gate_errors` from a caught `Err` as
        // "gates passed" previously reported `ok: true` while silently
        // skipping every hard gate.
        let (gate_errors, gate_check_failed) = if validation.valid {
            match ops::migrate_and_deserialize_graph(graph_json) {
                Ok(graph) => (ops::run_builder_gates(&self.config, &graph).await, false),
                Err(e) => {
                    tracing::warn!(
                        target: "flows",
                        error = %e,
                        "[flows] validate_workflow: graph passed structural validation but \
                         failed to migrate/deserialize for gate checks; failing closed"
                    );
                    (
                        vec![format!(
                            "hard gates could not run: graph failed to migrate/deserialize ({e})"
                        )],
                        true,
                    )
                }
            }
        } else {
            (Vec::new(), false)
        };

        let ok = validate_workflow_report_is_ok(validation.valid, &gate_errors, gate_check_failed);
        let report = json!({
            "ok": ok,
            "structurally_valid": validation.valid,
            "errors": validation.errors,
            "error_details": validation.error_details,
            "gate_errors": gate_errors,
            "warnings": validation.warnings,
        });
        Ok(ToolResult::success(serde_json::to_string_pretty(&report)?))
    }
}

/// `validate_workflow`'s aggregate verdict (T-m4): `ok` must be true only when
/// the graph is structurally valid, every hard gate ran, AND every hard gate
/// passed. Pulled out as a pure function so the fail-closed invariant — a
/// gate-check failure (e.g. a migrate/deserialize error) must never be
/// reported as `ok: true` — is unit-testable independent of the async gate
/// execution and the (currently unreachable, pending future per-node schema
/// migrations) path that produces `gate_check_failed`.
pub(super) fn validate_workflow_report_is_ok(
    structurally_valid: bool,
    gate_errors: &[String],
    gate_check_failed: bool,
) -> bool {
    structurally_valid && gate_errors.is_empty() && !gate_check_failed
}
