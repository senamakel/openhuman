//! Run control: resume a run parked on approval, cancel an in-flight run.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::Config;
use crate::flows::ops;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

// ─────────────────────────────────────────────────────────────────────────────
// Phase 4 — the self-debug loop + gated create (F4, F7)
// ─────────────────────────────────────────────────────────────────────────────

/// `resume_flow_run`: progress a run parked on a human approval by
/// approving/rejecting its pending node(s). Execute + approval-gated — it
/// advances a REAL run that can fire real outbound effects.
pub struct ResumeFlowRunTool {
    config: Arc<Config>,
}

impl ResumeFlowRunTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for ResumeFlowRunTool {
    fn name(&self) -> &str {
        "resume_flow_run"
    }

    fn description(&self) -> &str {
        "Resume a flow run that is paused on a human approval, approving and/or rejecting its \
         pending node(s). This ADVANCES A REAL RUN — approved outbound nodes will fire — so it is \
         approval-gated. Params: { flow_id, run_id, approve?: [node_id...], reject?: [node_id...] }. \
         Use list_flow_runs / get_flow_run to find a run with status pending_approval and its \
         pending node ids first."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "flow_id": { "type": "string", "description": "The run's flow id." },
                "run_id": { "type": "string", "description": "The run (thread) id to resume (from list_flow_runs)." },
                "approve": { "type": "array", "items": { "type": "string" }, "description": "Node ids to approve." },
                "reject": { "type": "array", "items": { "type": "string" }, "description": "Node ids to reject." }
            },
            "required": ["flow_id", "run_id"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Advances a real run (approved nodes fire) — gate like an execute-class,
        // approval-parked action.
        PermissionLevel::Execute
    }

    fn external_effect(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let flow_id = match args.get("flow_id").and_then(Value::as_str).map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return Ok(ToolResult::error("Missing 'flow_id' parameter".to_string())),
        };
        let run_id = match args.get("run_id").and_then(Value::as_str).map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return Ok(ToolResult::error("Missing 'run_id' parameter".to_string())),
        };
        let approve = string_array(&args, "approve");
        let reject = string_array(&args, "reject");
        tracing::debug!(target: "flows", %flow_id, %run_id, approve = approve.len(), reject = reject.len(), "[flows] resume_flow_run: resuming parked run");
        match ops::flows_resume(&self.config, &flow_id, &run_id, approve, reject).await {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string_pretty(
                &outcome.value,
            )?)),
            Err(e) => Ok(ToolResult::error(format!("Could not resume run: {e}"))),
        }
    }
}

/// `cancel_flow_run`: stop an in-flight or parked run. Write-class — it changes
/// run state but fires no new outbound effect.
///
/// **T-M3 fix.** This tool used to cancel an arbitrary `run_id` with no
/// ownership check at all — combined with `external_effect() == false` (so
/// the approval gate never parked it) and hiding that only covered the two
/// `flows_build` copilot/headless paths (`FLOWS_BUILD_COPILOT_HIDDEN_TOOLS`,
/// not the orchestrator-delegation or main-chat paths that also carry this
/// tool), a prompt-injected turn could cancel ANY user's in-flight or
/// approval-parked automation, unapproved. Two independent closes now apply:
/// 1. **Ownership check** — the caller must name the `flow_id` it believes
///    owns the run (mirrors [`ResumeFlowRunTool`]'s existing `{ flow_id,
///    run_id }` shape); the run row's *actual* `flow_id` is resolved and
///    compared, and a mismatch is refused rather than silently cancelling a
///    run scoped to a different flow.
/// 2. **`external_effect() == true`** — parks for approval on any surface
///    that has a gate, same as `resume_flow_run`.
pub struct CancelFlowRunTool {
    config: Arc<Config>,
}

impl CancelFlowRunTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CancelFlowRunTool {
    fn name(&self) -> &str {
        "cancel_flow_run"
    }

    fn description(&self) -> &str {
        "Cancel an in-flight or approval-parked flow run by its run_id (from list_flow_runs). \
         Stops a runaway or stuck run; fires no new outbound effect. The run_id must belong to \
         the given flow_id — cancelling a run that belongs to a different flow is refused. \
         Approval-gated. Params: { flow_id, run_id }."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "flow_id": { "type": "string", "description": "The flow that owns the run being cancelled (from list_flow_runs)." },
                "run_id": { "type": "string", "description": "The run (thread) id to cancel." }
            },
            "required": ["flow_id", "run_id"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn external_effect(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let flow_id = match args.get("flow_id").and_then(Value::as_str).map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return Ok(ToolResult::error("Missing 'flow_id' parameter".to_string())),
        };
        let run_id = match args.get("run_id").and_then(Value::as_str).map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return Ok(ToolResult::error("Missing 'run_id' parameter".to_string())),
        };

        // SECURITY (T-M3 fix): verify the run actually belongs to the
        // caller-named flow before cancelling anything — mirrors
        // `resume_flow_run` (`ops::flows_resume`)'s existing `run_record.flow_id
        // != flow_id` guard. Without this, any run_id (guessed, enumerated, or
        // named by a prompt-injected turn that never called list_flow_runs)
        // could cancel a run scoped to a completely different flow.
        let run = match ops::flows_get_run(&self.config, &run_id).await {
            Ok(outcome) => outcome.value,
            Err(e) => return Ok(ToolResult::error(format!("Could not cancel run: {e}"))),
        };
        if run.flow_id != flow_id {
            tracing::warn!(
                target: "flows",
                %flow_id,
                %run_id,
                actual_flow_id = %run.flow_id,
                "[flows] cancel_flow_run: refused — run belongs to a different flow than the one named"
            );
            return Ok(ToolResult::error(format!(
                "run '{run_id}' belongs to flow '{}', not '{flow_id}' — refusing to cancel",
                run.flow_id
            )));
        }

        tracing::debug!(target: "flows", %flow_id, %run_id, "[flows] cancel_flow_run: cancelling run");
        match ops::flows_cancel_run(&self.config, &run_id).await {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string_pretty(
                &outcome.value,
            )?)),
            Err(e) => Ok(ToolResult::error(format!("Could not cancel run: {e}"))),
        }
    }
}

/// Extracts a string array from `args[key]`, ignoring non-strings; empty when
/// absent. Shared by the resume tool's approve/reject lists.
fn string_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
