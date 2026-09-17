//! `dry_run_workflow`: execute a DRAFT against tinyflows MOCK capabilities (ungated, F7).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinyflows::model::WorkflowGraph;

use crate::config::Config;
use crate::flows::ops;
use crate::flows::ops::validate_and_migrate_graph;
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

use super::dry_run_diagnostics::{
    find_upstream_condition, tool_call_arg_null_entries, tool_call_error_message, CapturingObserver,
};

/// Wall-clock bound on a single `dry_run_workflow` mock execution. A malformed
/// or pathological draft graph must never hang the agent tool-loop; the mock
/// capabilities are non-blocking echoes, so this is a generous safety net.
const DRY_RUN_TIMEOUT_SECS: u64 = 30;

// ─────────────────────────────────────────────────────────────────────────────
// dry_run_workflow — execute a DRAFT against MOCK capabilities (ungated, F7)
// ─────────────────────────────────────────────────────────────────────────────

/// `dry_run_workflow`: compile a **draft** graph and run it against tinyflows'
/// deterministic **mock** capabilities, returning the merged node-state output
/// so the builder can self-verify a proposal before presenting it.
///
/// **No real side effects:** the run is wired to
/// [`tinyflows::caps::mock::mock_capabilities`] — the LLM / tool / HTTP / code
/// capabilities are echo stubs, so nothing external ever fires regardless of
/// the graph. The output is explicitly labeled `sandbox: true`.
///
/// **Not autonomy-tier gated (F7):** `permission_level()` returns
/// [`PermissionLevel::None`], so this tool runs on EVERY tier, read-only
/// included — a read-only agent must be able to self-verify its own proposal.
/// This is intentional, not an oversight: the mock capabilities never touch a
/// real integration, so there is nothing for a tier gate to protect. See
/// `dry_run_allowed_under_readonly_tier` in `builder_tools_tests.rs` for the
/// pinned regression (an earlier draft of this tool *was* tier-gated via an
/// unused `SecurityPolicy` field; the field was dead code by the time it
/// shipped and was removed rather than wired up, since side-effect-free
/// simulation has no tier to gate against).
///
/// **Wiring preflight:** the mock tool invoker is wrapped in the host's
/// [`PreflightToolInvoker`](crate::flows::tinyflows::caps::PreflightToolInvoker),
/// so a Composio `tool_call` whose required arg is missing or `=`-resolved to
/// null fails the dry run with the same actionable, field-naming error a real
/// run would produce — the echo mocks alone would happily accept a null `to`.
///
/// **Null-resolution check (the "produces functionally-broken workflows" fix):**
/// a required arg can be present *and non-Composio* (a native `oh:` tool, or a
/// Composio arg the catalog has no cached schema for) and still be wired to a
/// `=`-expression that silently resolves to `null` — the preflight above only
/// catches a *missing/null Composio-required* arg, so a graph like that used to
/// dry-run green and then do nothing at runtime. The run is driven through
/// [`tinyflows::engine::run_with_observer`] with a [`CapturingObserver`] that
/// records every node's [`ExecutionStep::diagnostics`](tinyflows::observability::ExecutionStep)
/// — the `=`-expressions the vendored engine itself traced as null-resolved
/// (see `tinyflows::expr::resolve_traced`). After the run settles, every
/// diagnostic on a **`tool_call` node's `args.*` location** is collected; any
/// hit fails the dry run with `ok: false` and the offending
/// `{ node_id, location, expression }` list, rather than reporting `ok: true`
/// for a graph that would silently no-op. Diagnostics on any OTHER
/// `agent`-node config subfield are NOT fatal here — a null there degrades
/// output quality but doesn't break execution the way a null tool arg does.
///
/// **Agent-prompt null check:** the ONE `agent`-node diagnostic that IS fatal
/// is a null-resolved **`prompt` itself** (`location == "prompt"`) — `prompt`
/// is the node's only input channel to the completion, so a `null` there
/// means the agent runs with a completely EMPTY prompt (the root-cause bug
/// `config.input_context` and `ops::validate_binding_resolvability`'s static
/// gate both exist to prevent). Collected separately into
/// `agent_prompt_nulls` (`{ node_id, location, expression, suggestion }`) and
/// added to the same `ok: false` condition as `null_resolutions`.
///
/// **Agent-`input_context` null check:** the SAME treatment applies to a
/// null-resolved **`input_context`** (`location == "input_context"`) — since
/// #4590 this is the agent's primary upstream-data channel (the very field
/// `prompt`-embedded jq expressions were supposed to stop needing), so a
/// `null` here is just as execution-breaking as a null `prompt`: the agent
/// runs with no upstream data at all. Collected separately into
/// `agent_input_context_nulls` (`{ node_id, location, expression, suggestion }`,
/// mirroring `agent_prompt_nulls` exactly) and added to the same `ok: false`
/// condition as `null_resolutions`/`agent_prompt_nulls`.
///
/// **`on_error: continue`/`route` does not mask a `tool_call` failure either.**
/// Those policies convert an executor error (e.g. the required-arg preflight
/// rejecting a null arg) into a routed error ITEM so the *run* still completes
/// (`Ok(outcome)`) — the failing node's `ExecutionStep` carries an EMPTY
/// `diagnostics` (the null check above would miss it) but its `status` is
/// [`StepStatus::Error`](tinyflows::observability::StepStatus::Error). Every
/// such `tool_call` step is collected into `node_errors`
/// (`{ node_id, error }`, the error text read back out of the run's `output`
/// state — see [`tool_call_error_message`]) and fails the dry run the same as
/// a null resolution.
///
/// **Routing-divergence warning (B15's dry-run blind spot):** none of the
/// checks above see a node that never ran at all. An `agent`/`tool_call` node
/// downstream of a `condition` can be silently unexercised because the
/// sandbox's mock trigger payload has a different *shape* than a real
/// trigger's (e.g. a webhook's real JSON body vs. the dry run's `{}`
/// default), so the condition takes a different branch under mock data than
/// it would at runtime — a graph can dry-run `ok: true` while its most
/// data-dependent node was never actually checked. After the run settles,
/// every `agent`/`tool_call` node with no [`ExecutionStep`] in the
/// [`CapturingObserver`] is collected into `routing_divergence_warnings`
/// (`{ node_id, condition_node_id, message }`, `condition_node_id` naming the
/// nearest upstream `condition` node found by walking predecessors — see
/// [`find_upstream_condition`] — or `null` if none is found). This is a
/// **warning, not a hard reject**: it never flips `ok` to `false` by itself
/// (an unexercised branch can be entirely intentional), and is surfaced on
/// both the `ok: true` and `ok: false` result shapes so the caller can
/// double-check that node's wiring by hand.
pub struct DryRunWorkflowTool {
    config: Arc<Config>,
}

impl DryRunWorkflowTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for DryRunWorkflowTool {
    fn name(&self) -> &str {
        "dry_run_workflow"
    }

    fn description(&self) -> &str {
        "Dry-run a workflow graph in a SANDBOX to self-verify it before \
         proposing. Compiles the graph and executes it against MOCK capabilities \
         — every LLM / tool_call / http_request / code node returns a deterministic \
         echo, so NOTHING real happens (no messages sent, no code run). Returns the \
         simulated per-node output labeled as sandbox output. Use it to catch \
         wiring/routing mistakes; it does NOT prove real integrations work. Provide \
         the graph as exactly one of `draft_id` (a working draft), `flow_id` (a saved \
         flow), or inline `graph` (draft_id wins, then flow_id), plus an optional \
         `input`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "A working draft to simulate. Provide one of draft_id / flow_id / graph (draft_id wins)."
                },
                "flow_id": {
                    "type": "string",
                    "description": "A saved flow to simulate. Provide one of draft_id / flow_id / graph."
                },
                "graph": {
                    "type": "object",
                    "description": "An inline tinyflows WorkflowGraph to simulate: { nodes: [...], edges: [...] }. Provide one of draft_id / flow_id / graph.",
                    "properties": {
                        "nodes": { "type": "array" },
                        "edges": { "type": "array" }
                    },
                    "required": ["nodes", "edges"]
                },
                "input": {
                    "description": "Optional trigger input passed to the run (defaults to {})."
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        // Mock-only and side-effect-free: nothing external ever fires (all
        // capabilities are echo stubs). So it needs no elevated permission and
        // is available on EVERY tier, read-only included (audit F7) — a
        // read-only agent must be able to self-verify its own proposal.
        PermissionLevel::None
    }

    fn external_effect(&self) -> bool {
        // Mock capabilities only — no real outbound effect.
        false
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        // Graph source: exactly one of a working draft, a saved flow, or an
        // inline graph — same precedence (draft_id > flow_id > graph) as the
        // sibling validate/edit tools, so they all accept the same handles.
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
                        "Could not load draft '{id}' to dry-run: {e}"
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
                        "Could not load flow '{id}' to dry-run: {e}"
                    )));
                }
            },
            (None, None, Some(v)) => v.clone(),
            (None, None, None) => {
                return Ok(ToolResult::error(
                    "Provide one of `draft_id` (a working draft), `flow_id` (a saved flow), or \
                     `graph` (an inline graph) to dry-run."
                        .to_string(),
                ));
            }
        };
        let input = args.get("input").cloned().unwrap_or_else(|| json!({}));

        let graph: WorkflowGraph = match validate_and_migrate_graph(graph_json) {
            Ok(graph) => graph,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Cannot dry-run an invalid graph: {e}. Fix the graph first."
                )))
            }
        };

        tracing::debug!(
            target: "flows",
            node_count = graph.nodes.len(),
            "[flows] dry_run_workflow: compiling + running draft against MOCK capabilities"
        );

        let compiled = match tinyflows::compiler::compile(&graph) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Draft graph failed to compile: {e}"
                )))
            }
        };

        // Wire the schema-aware mock `AgentRunner` so a draft with `agent`
        // nodes exercises the agent-node path during the dry run instead of
        // erroring on a missing capability — the plain `mock_capabilities()`
        // leaves `agent: None`. No real agent turn fires; the mock runner is a
        // deterministic echo, same contract as the other sandbox mocks, except
        // it additionally honors `config.output_parser.schema` (see its doc)
        // so the null-resolution check below doesn't false-positive on an
        // agent node that correctly declared a schema.
        let mut caps = tinyflows::caps::mock::mock_capabilities_with_agent(
            crate::flows::tinyflows::caps::SchemaAwareMockAgentRunner,
        );
        // Plain agent nodes (no `agent_ref`) never reach the runner above —
        // the vendored `agent` node routes them to the `llm` slot instead (see
        // `SchemaAwareMockLlm`'s doc). Swap the vendored `MockLlm` echo for the
        // schema-aware mock so their `output_parser.schema` is honored too,
        // instead of the echo shape failing the sub-port's validation.
        caps.llm = std::sync::Arc::new(crate::flows::tinyflows::caps::SchemaAwareMockLlm);
        // Wiring preflight over the echo mocks (see the struct doc): required
        // Composio args must be present and non-null even in the sandbox.
        caps.tools = std::sync::Arc::new(crate::flows::tinyflows::caps::PreflightToolInvoker {
            config: self.config.clone(),
            inner: caps.tools.clone(),
        });

        // Which node ids are `tool_call` nodes — the null-resolution check
        // below is scoped to just these (see the struct doc: a null in an
        // `agent`'s prompt is not execution-breaking the way a null tool arg
        // is, so only `tool_call` diagnostics fail the dry run).
        let tool_call_node_ids: std::collections::HashSet<&str> = graph
            .nodes
            .iter()
            .filter(|node| node.kind == tinyflows::model::NodeKind::ToolCall)
            .map(|node| node.id.as_str())
            .collect();

        // Which node ids are `agent` nodes — scoped narrowly to the ONE
        // execution-breaking agent diagnostic: a null-resolved `prompt`
        // itself (see the struct doc's "agent prompt nulls" section). Every
        // OTHER agent-config subfield (e.g. a null inside `tools` args) stays
        // non-fatal here, same as before.
        let agent_node_ids: std::collections::HashSet<&str> = graph
            .nodes
            .iter()
            .filter(|node| node.kind == tinyflows::model::NodeKind::Agent)
            .map(|node| node.id.as_str())
            .collect();

        // Capture every node's execution diagnostics (null-resolved
        // `=`-expressions the engine itself traced — see
        // `tinyflows::expr::resolve_traced`) as the sandbox run executes, so
        // they can be inspected once the run settles.
        let observer = Arc::new(CapturingObserver::default());
        let observer_dyn: Arc<dyn tinyflows::observability::RunObserver> = observer.clone();
        let run = tinyflows::engine::run_with_observer(&compiled, input, &caps, &observer_dyn);
        let outcome = match tokio::time::timeout(
            std::time::Duration::from_secs(DRY_RUN_TIMEOUT_SECS),
            run,
        )
        .await
        {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(e)) => {
                // A `stop`-policy `tool_call` whose required arg resolved null
                // aborts the WHOLE run here (via `PreflightToolInvoker`), so
                // the honest per-field diagnostic never reaches the settled-run
                // `null_resolutions` path below. Recover it from the observer:
                // if the abort was caused by a required arg bound to an upstream
                // Composio `tool_call`'s output, the echo mock simply CAN'T
                // produce that field — so surface it as `unverifiable` rather
                // than letting the generic "required arg missing/null" text
                // (which sent the transcript agent re-wiring a correct binding
                // three times) stand alone. WS6.
                let unverifiable_bindings: Vec<Value> =
                    tool_call_arg_null_entries(&observer.steps(), &graph, &tool_call_node_ids)
                        .into_iter()
                        .filter(|entry| {
                            entry.get("unverifiable").and_then(Value::as_bool) == Some(true)
                        })
                        .collect();
                if !unverifiable_bindings.is_empty() {
                    tracing::debug!(
                        target: "flows",
                        error = %e,
                        unverifiable_count = unverifiable_bindings.len(),
                        "[flows] dry_run_workflow: sandbox run aborted on a Composio-upstream \
                         binding the echo mock cannot verify — surfacing it honestly"
                    );
                    return Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
                        "sandbox": true,
                        "ok": false,
                        "error": e.to_string(),
                        "unverifiable_bindings": unverifiable_bindings,
                        "note": "SANDBOX (mock) output — a tool_call node aborted because a \
                            required arg binds to the output of an upstream Composio tool_call, \
                            which the sandbox can only ECHO (it never produces real tool output \
                            fields). See unverifiable_bindings: each MAY already be wired \
                            correctly — confirm the path with get_tool_contract {{ slug }} \
                            (output_fields / primary_array_path; Composio results nest under \
                            .item.json.data.) or get_tool_output_sample {{ slug, args }} instead \
                            of re-wiring blindly. No real side effects occurred.",
                    }))?));
                }
                tracing::debug!(target: "flows", error = %e, "[flows] dry_run_workflow: sandbox run errored");
                return Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
                    "sandbox": true,
                    "ok": false,
                    "error": e.to_string(),
                    "note": "SANDBOX (mock) output — a node errored during simulation. No real side effects occurred.",
                }))?));
            }
            Err(_elapsed) => {
                return Ok(ToolResult::error(format!(
                    "Sandbox dry-run timed out after {DRY_RUN_TIMEOUT_SECS}s"
                )))
            }
        };

        // Collect every null-resolved `=`-expression that landed on a
        // `tool_call` node's `args.*` config path — the class of binding
        // mistake that "builds" (compiles, dry-runs against echo mocks) but
        // does nothing at runtime because the wired field never had a value.
        // Each entry is honest about WHY it resolved null: a binding to an
        // upstream Composio `tool_call`'s output is flagged `unverifiable`
        // (the echo mock can't produce real tool output fields) rather than
        // reported as a plain wiring mistake — see [`build_null_resolution_entry`].
        let null_resolutions: Vec<Value> =
            tool_call_arg_null_entries(&observer.steps(), &graph, &tool_call_node_ids);

        // Collect every null-resolved `agent`-node `prompt` — execution-
        // breaking in the same way a null `tool_call` arg is: `prompt` is the
        // node's ONLY input channel to the completion, so a `null` there
        // means the agent runs with an EMPTY prompt (the exact root-cause bug
        // `input_context` — and the static gate in
        // `ops::validate_binding_resolvability` — exist to prevent). Scoped
        // to the `location == "prompt"` diagnostic specifically: other
        // agent-config subfields (e.g. a null buried in `tools` args) stay
        // non-fatal here, same as before this check existed.
        let agent_prompt_nulls: Vec<Value> = observer
            .steps()
            .iter()
            .filter(|step| agent_node_ids.contains(step.node_id.as_str()))
            .flat_map(|step| {
                step.diagnostics
                    .iter()
                    .filter(|&diag| diag.location == "prompt")
                    .map(|diag| {
                        json!({
                            "node_id": step.node_id,
                            "location": diag.location,
                            "expression": diag.expression,
                            "suggestion": "Feed upstream data via input_context:\"=item\" and \
                                make the prompt a plain instruction.",
                        })
                    })
            })
            .collect();

        // Collect every null-resolved `agent`-node `input_context` — mirrors
        // `agent_prompt_nulls` exactly (see the struct doc's "Agent-
        // `input_context` null check" section): `input_context` has been the
        // agent's primary upstream-data channel since #4590, so a null
        // resolution here is just as execution-breaking as a null `prompt` —
        // the agent runs with no upstream data at all.
        let agent_input_context_nulls: Vec<Value> = observer
            .steps()
            .iter()
            .filter(|step| agent_node_ids.contains(step.node_id.as_str()))
            .flat_map(|step| {
                step.diagnostics
                    .iter()
                    .filter(|&diag| diag.location == "input_context")
                    .map(|diag| {
                        json!({
                            "node_id": step.node_id,
                            "location": diag.location,
                            "expression": diag.expression,
                            "suggestion": "Wire input_context from a real upstream field, e.g. \
                                \"=nodes.<node_id>.item.json.<field>\" (or \"=item\" off the \
                                trigger), not an expression that resolves to null.",
                        })
                    })
            })
            .collect();

        // Collect every `tool_call` node whose EXECUTOR errored (e.g. the
        // Composio required-arg preflight rejecting a missing/null arg) —
        // regardless of that node's `on_error` policy. A `"continue"`/`"route"`
        // policy converts the failure into a routed error ITEM and the run
        // still completes successfully (`Ok(outcome)`), so the naive
        // `null_resolutions` check above misses it entirely: the failing
        // node's `ExecutionStep` carries an EMPTY `diagnostics` (the engine
        // never got far enough to trace an `=`-expression — see
        // `tinyflows::engine`'s error-item path) even though the node
        // genuinely failed. Only `"stop"` (the default) fails the whole run —
        // and that's already caught above via `Ok(Err(e))` before this point,
        // so every `StepStatus::Error` step reachable here is exactly the
        // continue/route case. The error text itself isn't on the step (the
        // engine only attaches it to the routed error item), so it's read
        // back out of `outcome.output`.
        let node_errors: Vec<Value> = observer
            .steps()
            .iter()
            .filter(|step| {
                tool_call_node_ids.contains(step.node_id.as_str())
                    && matches!(step.status, tinyflows::observability::StepStatus::Error)
            })
            .map(|step| {
                let error =
                    tool_call_error_message(&outcome.output, &step.node_id).unwrap_or_else(|| {
                        format!(
                            "tool_call node '{}' failed during the sandbox run — its `on_error` \
                             policy turned the failure into routed/continued data instead of \
                             failing the whole dry run, but the underlying error still means the \
                             node is broken.",
                            step.node_id
                        )
                    });
                json!({ "node_id": step.node_id, "error": error })
            })
            .collect();

        // Routing-divergence blind spot (B15): an `agent`/`tool_call` node that
        // did NOT execute during the sandbox run at all — because an upstream
        // `condition` routed the mock trigger payload onto its OTHER branch —
        // is invisible to every check above (`null_resolutions` etc. only
        // inspect steps that ran). But the mock input's *shape* need not match
        // a real trigger's shape (a webhook's real JSON vs. the dry run's `{}`
        // default, say), so a condition that took the `false` branch under mock
        // data may well take `true` at runtime with real data — or vice versa.
        // Either way, the dry run silently never exercised the very node whose
        // wiring most needed checking. This is a WARNING, not a hard reject
        // (an unexercised branch can be entirely intentional), surfaced
        // alongside the other diagnostics so the caller can double-check the
        // wiring by hand.
        let executed_steps = observer.steps();
        let executed_node_ids: std::collections::HashSet<&str> = executed_steps
            .iter()
            .map(|step| step.node_id.as_str())
            .collect();
        let routing_divergence_warnings: Vec<Value> = graph
            .nodes
            .iter()
            .filter(|node| {
                node.kind != tinyflows::model::NodeKind::Trigger
                    && (agent_node_ids.contains(node.id.as_str())
                        || tool_call_node_ids.contains(node.id.as_str()))
                    && !executed_node_ids.contains(node.id.as_str())
            })
            .map(|node| {
                let condition_node_id = find_upstream_condition(&graph, &node.id);
                let message = match &condition_node_id {
                    Some(cid) => format!(
                        "Node '{}' did not execute in the dry run (condition '{}' routed to \
                         the other branch under mock data); verify the wiring — at runtime \
                         with real data it may route differently.",
                        node.id, cid
                    ),
                    None => format!(
                        "Node '{}' did not execute in the dry run (an upstream branch routed \
                         the mock data away from it); verify the wiring — at runtime with real \
                         data it may route differently.",
                        node.id
                    ),
                };
                json!({
                    "node_id": node.id,
                    "condition_node_id": condition_node_id,
                    "message": message,
                })
            })
            .collect();

        // Quiet, informational only (never a prompt, never a gate): the
        // ApprovalGate permissions a real run of this graph will need, so the
        // builder agent can tell the user what the save+enable card will ask
        // for — the card itself fires at save+enable, NOT during dry runs.
        let permissions_manifest =
            crate::flows::ops::compute_approval_manifest(&self.config, &graph).await;

        tracing::info!(
            target: "flows",
            node_count = graph.nodes.len(),
            pending_approvals = outcome.pending_approvals.len(),
            null_resolution_count = null_resolutions.len(),
            agent_prompt_null_count = agent_prompt_nulls.len(),
            agent_input_context_null_count = agent_input_context_nulls.len(),
            node_error_count = node_errors.len(),
            routing_divergence_warning_count = routing_divergence_warnings.len(),
            permissions_manifest_count = permissions_manifest.len(),
            "[flows] dry_run_workflow: sandbox run finished"
        );

        if !null_resolutions.is_empty()
            || !agent_prompt_nulls.is_empty()
            || !agent_input_context_nulls.is_empty()
            || !node_errors.is_empty()
        {
            tracing::debug!(
                target: "flows",
                ?null_resolutions,
                ?agent_prompt_nulls,
                ?agent_input_context_nulls,
                ?node_errors,
                "[flows] dry_run_workflow: tool_call/agent-prompt/agent-input_context issue(s) \
                 found — failing the dry run"
            );
            return Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
                "sandbox": true,
                "ok": false,
                "null_resolutions": null_resolutions,
                "agent_prompt_nulls": agent_prompt_nulls,
                "agent_input_context_nulls": agent_input_context_nulls,
                "node_errors": node_errors,
                "routing_divergence_warnings": routing_divergence_warnings,
                "permissions_manifest": permissions_manifest,
                "message": "These tool_call args resolved to null, an agent node's prompt or \
                    input_context resolved to null (an EMPTY prompt — see agent_prompt_nulls — \
                    or no upstream data at all — see agent_input_context_nulls), or a tool_call \
                    node failed during the sandbox run (even one recovered via on_error: \
                    continue/route) — wire null-resolved args from an upstream node's real \
                    output (give any agent node an output_parser.schema so its fields are \
                    addressable), feed upstream data into a null-resolved agent prompt/ \
                    input_context from a real upstream field instead of a jq expression inside \
                    the prompt text, and fix or rewire whatever tool_call node_errors names. Also \
                    check routing_divergence_warnings: any agent/tool_call node listed there \
                    never ran in this sandbox at all because an upstream condition routed the \
                    mock data past it — verify that wiring by hand too.",
            }))?));
        }

        Ok(ToolResult::success(serde_json::to_string_pretty(&json!({
            "sandbox": true,
            "ok": true,
            "output": outcome.output,
            "pending_approvals": outcome.pending_approvals,
            "null_resolutions": null_resolutions,
            "agent_prompt_nulls": agent_prompt_nulls,
            "agent_input_context_nulls": agent_input_context_nulls,
            "node_errors": node_errors,
            "routing_divergence_warnings": routing_divergence_warnings,
            "permissions_manifest": permissions_manifest,
            "note": "SANDBOX (mock) output — LLM/tool/HTTP/code nodes returned deterministic echoes; NO real side effects occurred. This checks wiring/routing only, not whether real integrations work. \
                If routing_divergence_warnings is non-empty, an agent/tool_call node never ran in \
                this sandbox because an upstream condition routed the mock data past it — that \
                node's wiring is unverified; check it by hand.",
        }))?))
    }
}
