//! Persistence tools: `create_workflow`, `duplicate_flow`, and `save_workflow`.
//! Created/duplicated flows are always born DISABLED; save never touches enablement.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::Config;
use crate::flows::ops;
use crate::flows::ops::validate_and_migrate_graph;
use crate::flows::tools;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

/// `create_workflow`: the gated create tool (audit F4/F12). Persists a NEW
/// flow, always **born disabled** (enable stays human-only) and behind the
/// forced `require_approval` floor for side-effect graphs. Write + approval
/// gated. This is the deliberate widening the Phase 3 rails (versioning,
/// events, history) make safe.
pub struct CreateWorkflowTool {
    config: Arc<Config>,
}

impl CreateWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for CreateWorkflowTool {
    fn name(&self) -> &str {
        "create_workflow"
    }

    fn description(&self) -> &str {
        "Create a NEW saved flow from a graph. Approval-gated. The flow is ALWAYS created DISABLED \
         (only the user can enable it via the UI) and inherits the forced approval gate for any \
         outbound action — so a created flow can never fire on its own without an explicit human \
         enable. Runs the same author hard-gates as save. Params: { name, graph, require_approval? }. \
         Prefer propose_workflow when the user just wants to review a design; use this when they've \
         explicitly asked you to create the flow."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Human-readable flow name." },
                "graph": {
                    "type": "object",
                    "description": "The tinyflows WorkflowGraph: { nodes: [...], edges: [...] }.",
                    "properties": { "nodes": { "type": "array" }, "edges": { "type": "array" } },
                    "required": ["nodes", "edges"]
                },
                "require_approval": { "type": "boolean", "description": "Force the approval gate (defaults true)." }
            },
            "required": ["name", "graph"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn external_effect(&self) -> bool {
        // Persists a new flow definition.
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let name = match args.get("name").and_then(Value::as_str).map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => return Ok(ToolResult::error("Missing 'name' parameter".to_string())),
        };
        let graph_json = match args.get("graph") {
            Some(v) if !v.is_null() => v.clone(),
            _ => return Ok(ToolResult::error("Missing 'graph' parameter".to_string())),
        };
        let require_approval = args
            .get("require_approval")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        // Same structural + hard-gate stack an agent save must pass.
        if let Err(msg) = ops::strict_gate(&self.config, &graph_json).await {
            return Ok(ToolResult::error(format!(
                "{msg}\n\nFix the graph and call create_workflow again."
            )));
        }

        tracing::info!(target: "flows", %name, "[flows] create_workflow: agent-initiated create (born disabled)");
        let flow = match ops::flows_create(&self.config, name, graph_json, require_approval).await {
            Ok(outcome) => outcome.value,
            Err(e) => return Ok(ToolResult::error(format!("Could not create flow: {e}"))),
        };

        // Force born-disabled: enable stays human-only, even for a manual-trigger
        // graph that flows_create would otherwise create enabled. `flows_create`
        // and this force-disable are two separate writes — not one transaction —
        // so there is necessarily a brief window between them where the row is
        // persisted `enabled: true` before this call disables it. This fix does
        // not close that window; it only stops MISREPORTING the outcome when the
        // disable itself fails.
        //
        // T-m3: `flows_set_enabled(.., false)` can fail (store error, flow
        // deleted concurrently, …). That used to be only `warn!`-logged while
        // the response unconditionally claimed `"enabled": false` — so a
        // manual-trigger flow that flows_create left enabled would stay
        // enabled while the agent told the user it was disabled. Track the
        // real post-attempt state and report THAT.
        let mut disable_succeeded = true;
        if flow.enabled {
            match ops::flows_set_enabled(&self.config, &flow.id, false).await {
                Ok(_) => {}
                Err(e) => {
                    disable_succeeded = false;
                    tracing::warn!(
                        target: "flows",
                        flow_id = %flow.id,
                        error = %e,
                        "[flows] create_workflow: could not force-disable the new flow — it \
                         remains ENABLED; reporting the true state, not the intended one"
                    );
                }
            }
        }
        let (enabled, note) = create_workflow_report(flow.enabled, disable_succeeded);

        Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
            "type": "workflow_created",
            "flow_id": flow.id,
            "name": flow.name,
            "enabled": enabled,
            "require_approval": flow.require_approval,
            "note": note,
        }))?))
    }
}

/// `create_workflow`'s reported `enabled` state + note (T-m3): derived from
/// whether the flow was born enabled (`born_enabled`, from `flows_create`'s
/// Rule 1) and whether the subsequent force-disable attempt succeeded
/// (`disable_succeeded`, ignored when no attempt was made). Pulled out as a
/// pure function so the fail-HONEST invariant — the response must reflect
/// the flow's real post-attempt state, not the intended one — is
/// unit-testable without forcing a genuine concurrent store failure between
/// `flows_create` and `flows_set_enabled`.
pub(super) fn create_workflow_report(
    born_enabled: bool,
    disable_succeeded: bool,
) -> (bool, &'static str) {
    let enabled = born_enabled && !disable_succeeded;
    let note = if enabled {
        "Flow created, but it could NOT be force-disabled (see the tool result for the \
         underlying error) — it is currently ENABLED. Tell the user and ask them to disable it \
         manually if that was not intended."
    } else {
        "Flow created DISABLED. The user must enable it explicitly before it can run."
    };
    (enabled, note)
}

/// `duplicate_flow`: create an independent, DISABLED copy of a saved flow — the
/// clone-then-edit pattern. Write-class.
pub struct DuplicateFlowTool {
    config: Arc<Config>,
}

impl DuplicateFlowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for DuplicateFlowTool {
    fn name(&self) -> &str {
        "duplicate_flow"
    }

    fn description(&self) -> &str {
        "Duplicate a saved flow: create an independent, DISABLED copy of its graph under a new id \
         (name suffixed \" (copy)\"). The copy never fires until the user enables it. Use this for \
         the clone-then-edit pattern (edit_workflow the copy). Params: { flow_id }."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "flow_id": { "type": "string", "description": "The saved flow to duplicate." } },
            "required": ["flow_id"],
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
        tracing::info!(target: "flows", %flow_id, "[flows] duplicate_flow: agent-initiated duplicate");
        match ops::flows_duplicate(&self.config, &flow_id).await {
            Ok(outcome) => {
                let flow = outcome.value;
                Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
                    "type": "workflow_duplicated",
                    "flow_id": flow.id,
                    "name": flow.name,
                    "enabled": flow.enabled,
                }))?))
            }
            Err(e) => Ok(ToolResult::error(format!("Could not duplicate flow: {e}"))),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// save_workflow — persist a built graph onto an EXISTING saved flow
// ─────────────────────────────────────────────────────────────────────────────

/// `save_workflow`: persist a validated graph (and optionally a new name) onto
/// an **existing, already-saved** flow via [`ops::flows_update`] — the same
/// validate-and-migrate path the UI's Save uses.
///
/// It was originally added as a narrow, deliberate exception to the belt's
/// "propose, never persist" invariant (for the Flows prompt bar's
/// instant-create path, where the host creates the flow *before* delegating
/// and hands the agent its `flow_id`) — before [`CreateWorkflowTool`] and
/// [`DuplicateFlowTool`] existed, this was the belt's only write. Both now
/// exist, so `save_workflow` is one of three persistence tools, not the sole
/// one. Its own remaining boundaries:
///
/// - **Update-only.** It requires an existing `flow_id`; it never fabricates
///   one. Creating a flow is [`CreateWorkflowTool`]/[`DuplicateFlowTool`]'s
///   job — `save_workflow` can only write onto a flow that already exists
///   (whether the host, the user, or an earlier `create_workflow`/
///   `duplicate_flow` call made it).
/// - **Never touches enablement or the approval gate.** `enabled` and
///   `require_approval` are not parameters; whatever the user set stays —
///   except that saving a graph whose trigger just transitioned from manual
///   to automatic on an already-enabled flow auto-disables it (see
///   [`ops::flows_update`]'s own doc for that guard).
/// - **Real persistence, real consequences.** Saving a `schedule`/`app_event`
///   trigger onto an ENABLED flow arms it (the trigger binds and will fire on
///   its own) — hence `PermissionLevel::Write`. The description tells the agent
///   to dry-run first and to say what it saved.
pub struct SaveWorkflowTool {
    config: Arc<Config>,
}

impl SaveWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for SaveWorkflowTool {
    fn name(&self) -> &str {
        "save_workflow"
    }

    fn description(&self) -> &str {
        "Save a workflow graph onto an EXISTING saved flow (by `flow_id`), persisting it. \
         This is the ONLY builder tool that writes onto a saved flow — edit/validate/dry_run \
         never do. Use it after the user asked you to build/update a workflow and you have \
         dry-run-verified the graph. The graph source is either `draft_id` (a working draft — \
         the usual case after editing with edit_workflow; draft_id wins if both are given) or \
         an inline `graph`; `flow_id` is always required as the persistence TARGET. It \
         validates and writes the graph (and optional new `name`) to that flow. It can NOT \
         create a new flow, and it never touches the approval gate — but it CAN \
         auto-disable the flow when the trigger transitions from manual to automatic \
         (schedule/webhook/app_event), so a save never silently arms a trigger that wasn't \
         already live; the returned `warnings` will explain it when that happens. NOTE: if \
         the flow was ALREADY enabled with an automatic trigger and stays automatic, saving \
         re-arms it live — it will start firing on its own. Always tell the user what you \
         saved (including any auto-disable). Params: { flow_id, draft_id? | graph?, name? }."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "flow_id": {
                    "type": "string",
                    "description": "Id of the EXISTING saved flow to write the graph to (the persistence target — always required)."
                },
                "draft_id": {
                    "type": "string",
                    "description": "A working draft whose graph to persist onto the flow. Provide this OR inline `graph`; if both are given, draft_id wins."
                },
                "graph": {
                    "type": "object",
                    "description": "The full tinyflows WorkflowGraph to persist: { name?, nodes: [...], edges: [...] }. Provide this OR `draft_id`. Same shape as propose_workflow.",
                    "properties": {
                        "nodes": { "type": "array" },
                        "edges": { "type": "array" }
                    },
                    "required": ["nodes", "edges"]
                },
                "name": {
                    "type": "string",
                    "description": "Optional new human-readable name for the flow."
                }
            },
            "required": ["flow_id"],
            "additionalProperties": false
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Persists a flow definition; on an enabled flow this can arm a
        // self-firing trigger — gate like a Write-class action.
        PermissionLevel::Write
    }

    fn external_effect(&self) -> bool {
        // Persistence is local (no message/HTTP/code fires at save time); the
        // flow's own runs — and their approval gate — govern real effects.
        false
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let flow_id = match args.get("flow_id").and_then(Value::as_str).map(str::trim) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                return Ok(ToolResult::error(
                    "Missing 'flow_id' — save_workflow only updates an EXISTING saved flow. \
                     If there is no flow yet, return the proposal and let the user save it."
                        .to_string(),
                ))
            }
        };
        // Graph source: a working draft (the usual post-edit_workflow handle) or
        // an inline graph. `flow_id` above is the persistence TARGET, always
        // required; the draft only supplies the graph to write. If both a
        // draft_id and an inline graph are given, the draft wins (it is the
        // durable working copy the agent just iterated on).
        let draft_id = args
            .get("draft_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let graph_json =
            if let Some(id) = draft_id {
                match ops::flows_draft_get(&self.config, id) {
                    Ok(outcome) => outcome.value.graph,
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "Could not load draft '{id}' to save: {e}"
                        )));
                    }
                }
            } else {
                match args.get("graph") {
                    Some(v) if !v.is_null() => v.clone(),
                    _ => return Ok(ToolResult::error(
                        "Provide `draft_id` (a working draft) or inline `graph` to save onto the \
                         flow."
                            .to_string(),
                    )),
                }
            };
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        // Same migrate/validate + enforcing binding-resolvability gate as
        // propose_workflow/revise_workflow, run HERE at the tool level (not
        // inside `ops::flows_update`, which the UI/RPC also call for a
        // human's own edits and which must stay permissive) — so an agent
        // can never persist a graph with an unresolvable `tool_call` binding
        // either. See `ops::validate_binding_resolvability`.
        let graph = match validate_and_migrate_graph(graph_json.clone()) {
            Ok(graph) => graph,
            Err(e) => {
                tracing::debug!(target: "flows", %flow_id, error = %e, "[flows] save_workflow: validation failed");
                return Ok(ToolResult::error(format!(
                    "Workflow graph is invalid: {e}. Fix the graph and call save_workflow again."
                )));
            }
        };
        // The full builder hard-gate stack, run through the single canonical
        // runner shared with propose/revise/edit and the strict create/update
        // RPC path (F3) — so an agent can never persist a graph that would fail
        // gates the other planes enforce.
        let gate_errors = ops::run_builder_gates(&self.config, &graph).await;
        if !gate_errors.is_empty() {
            tracing::debug!(
                target: "flows",
                %flow_id,
                error_count = gate_errors.len(),
                "[flows] save_workflow: a hard gate rejected the graph"
            );
            return Ok(ToolResult::error(format!(
                "{}\n\nFix these and call save_workflow again.",
                gate_errors.join("\n\n")
            )));
        }
        // Author-time warnings (unfired trigger kinds + unwired REQUIRED
        // Composio args) were previously computed by propose/revise but never
        // surfaced again at save time — add them here so the agent sees any
        // non-fatal wiring gaps that remain in the final persisted graph.
        let mut warnings = ops::graph_trigger_warnings(&graph);
        warnings.extend(ops::graph_wiring_warnings(&self.config, &graph).await);

        tracing::info!(
            target: "flows",
            %flow_id,
            renaming = name.is_some(),
            "[flows] save_workflow: agent-initiated save to existing flow"
        );

        match ops::flows_update(&self.config, &flow_id, name, Some(graph_json), None, None).await {
            Ok(outcome) => {
                let flow = outcome.value;
                tracing::info!(
                    target: "flows",
                    %flow_id,
                    node_count = flow.graph.nodes.len(),
                    enabled = flow.enabled,
                    "[flows] save_workflow: persisted"
                );
                // Surface any explanatory logs `flows_update` produced — most
                // notably the manual→automatic auto-disarm message (#4889) —
                // to the agent. Skip the boilerplate "flow updated: <id>" line,
                // which just duplicates the `persisted`/`flow_id` fields this
                // response already carries.
                let flow_updated_boilerplate = format!("flow updated: {flow_id}");
                warnings.extend(
                    outcome
                        .logs
                        .into_iter()
                        .filter(|log| *log != flow_updated_boilerplate),
                );
                // Issue B29 (save/enable safety), Rule 3: `flows_create` only
                // gates the FIRST creation of a flow — an agent `save_workflow`
                // targets an EXISTING flow via `flows_update`, which (since
                // #4889) force-disables the flow whenever the trigger
                // transitions from manual to automatic (schedule/webhook/
                // app_event) — so a save can never silently arm a trigger that
                // wasn't already live (see the `warnings.extend` above for the
                // explanatory log). Short of that transition, `flows_update`
                // preserves whatever `enabled` state the flow already had: if
                // it was ALREADY enabled with an automatic trigger and stays
                // automatic, saving a new graph onto it re-arms it live with no
                // further confirmation. Surface that loudly so the copilot
                // relays it to the user instead of staying silent.
                if flow.enabled && ops::trigger_is_automatic(&flow.graph) {
                    let trigger_desc = flow
                        .graph
                        .trigger()
                        .map(tools::describe_trigger)
                        .unwrap_or_else(|| "automatic".to_string());
                    let warning = format!(
                        "WARNING: this flow is ENABLED with an automatic trigger \
                         ({trigger_desc}). It is now LIVE and will fire on its own — tell the \
                         user, and offer to disable it (flows_set_enabled) if that's not what \
                         they intended."
                    );
                    tracing::warn!(
                        target: "flows",
                        %flow_id,
                        trigger = %trigger_desc,
                        "[flows] save_workflow: saved onto an enabled auto-trigger flow — now LIVE"
                    );
                    warnings.push(warning);
                }
                Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
                    "type": "workflow_saved",
                    // Explicit counterpart to a proposal's persisted:false — this
                    // graph IS now written onto the saved flow.
                    "persisted": true,
                    "flow_id": flow.id,
                    "name": flow.name,
                    "enabled": flow.enabled,
                    "require_approval": flow.require_approval,
                    "node_count": flow.graph.nodes.len(),
                    "warnings": warnings,
                }))?))
            }
            Err(e) => {
                tracing::debug!(target: "flows", %flow_id, error = %e, "[flows] save_workflow: failed");
                Ok(ToolResult::error(format!(
                    "Could not save workflow to flow '{flow_id}': {e}"
                )))
            }
        }
    }
}
