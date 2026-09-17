//! Diagnostic helpers for `dry_run_workflow`: null-resolution entries, upstream
//! condition lookup, routed tool-call error extraction, and the capturing observer.

use serde_json::{json, Value};
use tinyflows::model::WorkflowGraph;

/// Builds one `null_resolutions` diagnostic entry for a `tool_call` node's
/// null-resolved `args.*` config expression.
///
/// The common case reports `{ node_id, location, expression }` — a wiring
/// mistake the agent should fix. But when the null-resolved expression binds to
/// the output of an upstream Composio-or-native `tool_call` node
/// ([`ops::mock_opaque_tool_call_upstream_ref`]), the entry is instead marked
/// `unverifiable: true` and carries an honest `suggestion`: the echo sandbox
/// can NEVER produce a tool's real output fields, so this particular null is
/// expected here and does NOT prove the binding wrong (WS6 — the transcript
/// audit where the agent re-wired an already-correct binding three times
/// chasing this exact false negative). The suggestion adapts to the upstream
/// kind: a Composio upstream points at `get_tool_contract` /
/// `get_tool_output_sample` and the `.item.json.data.` nesting; a native `oh:`
/// upstream points at the flat `.item.json.<field>` shape instead.
fn build_null_resolution_entry(
    node_id: &str,
    diag: &tinyflows::expr::NullResolution,
    graph: &WorkflowGraph,
) -> Value {
    if let Some(upstream) =
        tinyflows::preflight::mock_opaque_tool_call_upstream_ref(&diag.expression, graph, node_id)
    {
        let field = diag.location.strip_prefix("args.").unwrap_or("args");
        // The disambiguation advice differs by upstream kind: a native `oh:`
        // tool's output binds FLAT (`.item.json.<field>`) after
        // `native_tool_payload`'s unwrap — it has no `.data.` wrapper and no
        // Composio `get_tool_contract` — whereas a Composio action nests under
        // `.item.json.data.`. Emitting the Composio advice for a native
        // upstream would send the agent chasing a `.data.` path that will
        // never exist.
        let upstream_is_native = graph
            .nodes
            .iter()
            .find(|n| n.id == upstream)
            .and_then(|n| n.config.get("slug").and_then(Value::as_str))
            .is_some_and(|s| s.starts_with("oh:"));
        let suggestion = if upstream_is_native {
            format!(
                "required arg `{field}` binds to the output of native tool_call node \
                 `{upstream}` — the SANDBOX only echoes tool calls and can never produce \
                 their real output fields, so this binding is UNVERIFIABLE here (not \
                 necessarily wrong). A native `oh:` tool's real output binds FLAT at \
                 `=nodes.{upstream}.item.json.<field>` (no `.data.` wrapper). Confirm the \
                 field name against that tool's own output shape. It is a real bug only if \
                 the path doesn't match the tool's actual output."
            )
        } else {
            format!(
                "required arg `{field}` binds to the output of Composio tool_call node \
                 `{upstream}` — the SANDBOX only echoes tool calls and can never produce \
                 their real output fields, so this binding is UNVERIFIABLE here (not \
                 necessarily wrong). Confirm the path against get_tool_contract {{ slug }}'s \
                 output_fields / primary_array_path (remember Composio results nest under \
                 `.item.json.data.`), or get_tool_output_sample {{ slug, args }} for the \
                 real shape. It is a real bug only if the path doesn't match the action's \
                 actual output."
            )
        };
        return json!({
            "node_id": node_id,
            "location": diag.location,
            "expression": diag.expression,
            "unverifiable": true,
            "upstream_tool_call": upstream,
            "suggestion": suggestion,
        });
    }
    json!({
        "node_id": node_id,
        "location": diag.location,
        "expression": diag.expression,
    })
}

/// Every null-resolved `args.*` config expression that landed on a `tool_call`
/// node, as `null_resolutions` diagnostic entries (see
/// [`build_null_resolution_entry`] for the shape, including the WS6
/// `unverifiable` Composio-or-native-upstream variant). Shared by the settled-run path
/// (which fails the dry run on these) and the errored-run path (which surfaces
/// only the `unverifiable` ones so a stop-policy preflight abort explains
/// itself honestly instead of via the generic required-arg text).
pub(super) fn tool_call_arg_null_entries(
    steps: &[tinyflows::observability::ExecutionStep],
    graph: &WorkflowGraph,
    tool_call_node_ids: &std::collections::HashSet<&str>,
) -> Vec<Value> {
    steps
        .iter()
        .filter(|step| tool_call_node_ids.contains(step.node_id.as_str()))
        .flat_map(|step| {
            step.diagnostics
                .iter()
                .filter(|&diag| diag.location == "args" || diag.location.starts_with("args."))
                .map(|diag| build_null_resolution_entry(&step.node_id, diag, graph))
        })
        .collect()
}

/// Walks a graph backward from `node_id`'s predecessors (any number of hops)
/// to find the nearest ancestor that is a `condition` node — used to name the
/// branch responsible for a routing-divergence warning (see
/// [`DryRunWorkflowTool::execute`]'s routing-divergence check, just above).
/// Returns `None` if no predecessor chain reaches a `condition` node (e.g. the
/// node simply has no predecessors, or none of them is a condition) — the
/// warning is still emitted, just without a named culprit node.
pub(super) fn find_upstream_condition(graph: &WorkflowGraph, node_id: &str) -> Option<String> {
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<&str> = graph
        .edges
        .iter()
        .filter(|edge| edge.to_node == node_id)
        .map(|edge| edge.from_node.as_str())
        .collect();
    while let Some(current) = queue.pop_front() {
        if !visited.insert(current) {
            continue;
        }
        if let Some(node) = graph.nodes.iter().find(|n| n.id == current) {
            if node.kind == tinyflows::model::NodeKind::Condition {
                return Some(node.id.clone());
            }
        }
        for edge in graph.edges.iter().filter(|edge| edge.to_node == current) {
            queue.push_back(edge.from_node.as_str());
        }
    }
    None
}

/// Best-effort extraction of the human-readable error message the engine
/// recorded for a `tool_call` node whose `on_error` policy is `"continue"` or
/// `"route"`. Such a node's failure is converted into an error ITEM on its
/// output (`{ "error": { "message", "node" } }` — see `tinyflows::engine`'s
/// `error_item`) rather than failing the whole run, so the message lives in
/// the run's `output` state, not on the [`tinyflows::observability::ExecutionStep`]
/// itself (whose `diagnostics` stays empty for an error step — see
/// [`DryRunWorkflowTool::execute`]'s `node_errors` collection).
pub(super) fn tool_call_error_message(output: &Value, node_id: &str) -> Option<String> {
    output
        .get("nodes")?
        .get(node_id)?
        .get("items")?
        .as_array()?
        .iter()
        .find_map(|item| {
            item.get("json")?
                .get("error")?
                .get("message")?
                .as_str()
                .map(str::to_string)
        })
}

/// The engine's own step-capturing observer, re-exported under the name
/// [`DryRunWorkflowTool`]'s call sites already use.
///
/// It is upstream because what it captures is the engine's:
/// `ExecutionStep::diagnostics` holds the `=`-expressions that resolved to null
/// while a node's config was being assembled, which is the only place a graph's
/// real wiring failure is visible. This host used to declare an identical copy.
pub(crate) use tinyflows::observability::CapturingObserver;
