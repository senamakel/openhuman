//! The [`Flow`] entity: a saved automation workflow definition.
//!
//! Wraps `tinyflows::model::WorkflowGraph` with the metadata OpenHuman needs to
//! store, list, and track runs for a saved flow. The graph itself is the
//! portable, tinyflows-owned contract (validated + migrated on load); this
//! struct is the OpenHuman-side record around it.

use serde::{Deserialize, Serialize};
use tinyflows::model::WorkflowGraph;

/// How a flow run was started. Stamped onto the run's Langfuse trace as a
/// `trigger:<kind>` tag plus `trigger` metadata so runs can be filtered by
/// origin in the Langfuse UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowRunTrigger {
    /// An explicit run request over RPC/CLI (the Workflows UI "Run" button).
    Rpc,
    /// A `FlowScheduleTick` cron dispatch (`schedule` trigger node).
    Schedule,
    /// A `ComposioTriggerReceived` dispatch (`app_event` trigger node).
    AppEvent,
    /// A human-in-the-loop resume of a paused run (`flows_resume`).
    Resume,
}

impl FlowRunTrigger {
    /// Stable snake_case identifier used in Langfuse tags/metadata.
    pub fn as_str(&self) -> &'static str {
        match self {
            FlowRunTrigger::Rpc => "rpc",
            FlowRunTrigger::Schedule => "schedule",
            FlowRunTrigger::AppEvent => "app_event",
            FlowRunTrigger::Resume => "resume",
        }
    }
}

/// The result of validating a candidate `tinyflows` graph without persisting
/// it — returned by `openhuman.flows_validate` (PHASE 3c) and used to surface
/// structural errors and non-fatal warnings (e.g. "this trigger kind never
/// fires automatically yet") to an authoring surface *before* a flow is saved.
///
/// A graph is `valid` when it passes `tinyflows::validate::validate` after
/// migration; `errors` carries the single structural error when it does not.
/// `warnings` is orthogonal to validity — a `valid` graph can still carry
/// warnings (it saves and enables fine, it just won't behave as an author
/// might expect), and an invalid graph reports no warnings (there's nothing to
/// warn about a graph that won't compile).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FlowValidation {
    /// True when the graph is structurally valid (migrates + validates).
    pub valid: bool,
    /// Structural validation errors (empty when `valid`). Today at most one —
    /// `tinyflows::validate::validate` returns the first error it hits.
    pub errors: Vec<String>,
    /// Non-fatal warnings: the graph is accepted, but something about it is
    /// worth flagging (e.g. an unfired trigger kind). Never blocks save/enable.
    pub warnings: Vec<String>,
}

/// A saved automation workflow: a `tinyflows` graph plus OpenHuman-side
/// bookkeeping (enablement, run history summary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    /// Stable identifier (UUID) for this flow.
    pub id: String,
    /// Human-readable name shown in the Workflows UI.
    pub name: String,
    /// Whether this flow may currently be triggered (B2) / run.
    pub enabled: bool,
    /// The validated, migrated workflow graph.
    pub graph: WorkflowGraph,
    /// RFC3339 creation timestamp.
    pub created_at: String,
    /// RFC3339 last-update timestamp.
    pub updated_at: String,
    /// RFC3339 timestamp of the most recent run, if any.
    pub last_run_at: Option<String>,
    /// Outcome of the most recent run: `"completed"` | `"pending_approval"` | `"failed"`.
    pub last_status: Option<String>,
    /// "Require approval for outbound actions" (issue B2). When `true`, the
    /// approval gate does NOT auto-allow this flow's `TrustedAutomation
    /// { Workflow }` trust root — every external_effect tool/HTTP call the
    /// flow makes still parks for a real decision, regardless of how the run
    /// was triggered. See `src/openhuman/approval/gate.rs` and
    /// `src/openhuman/agent/turn_origin.rs::TrustedAutomationSource::Workflow`.
    #[serde(default)]
    pub require_approval: bool,
}

/// One step of a persisted [`FlowRun`] (run-history inspector).
///
/// As of issue G2 (live run observation) these are persisted **incrementally**
/// as each non-trigger node finishes, by
/// `flows::observability::FlowRunObserver::on_step_finish`, which maps a live
/// `tinyflows::observability::ExecutionStep` (carrying real `status` +
/// `duration_ms`) onto this type. The prior post-hoc reconstruction from
/// `RunOutcome.output["nodes"]` (see `flows::ops::reconstruct_steps`) now only
/// fills in steps the observer missed (e.g. a trigger node, which does not
/// emit an `on_step_finish`) — those carry no `status`/`duration_ms` and keep
/// the `port` the reconstruction recovers.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct FlowRunStep {
    /// The node's id within the flow's graph.
    pub node_id: String,
    /// The node's emitted items for this run (`output["nodes"][id]["items"]`,
    /// or the live `ExecutionStep.output` when observed incrementally).
    pub output: serde_json::Value,
    /// The output port the node routed on, if it picked one (branching /
    /// switch nodes) — `output["nodes"][id]["port"]`. Only recovered by the
    /// post-hoc reconstruction; the live observer does not carry a port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// Live step outcome, when this step was observed incrementally:
    /// `"success"` | `"error"`. `None` for a step recovered post-hoc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Wall-clock duration of the node's executor in milliseconds, when
    /// observed incrementally. `None` for a step recovered post-hoc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// A persisted record of one `flows_run` / `flows_resume` invocation, for the
/// B3 run-history inspector. Written by `flows::store` from `flows::ops`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowRun {
    /// Stable identifier for this run — the same value as `thread_id` (the
    /// tinyflows checkpointer key), so a run row can be found either way.
    pub id: String,
    /// The flow this run belongs to.
    pub flow_id: String,
    /// The tinyflows checkpointer thread id (needed to `flows_resume`).
    pub thread_id: String,
    /// Run status. Not an enum (kept a free-form `String` for forward-compat
    /// with statuses added by newer builds), but the vocabulary is fixed:
    /// `"running"` | `"completed"` | `"pending_approval"` | `"failed"` |
    /// `"cancelled"` (issue G4 — a run cancelled via `flows_cancel_run`, or a
    /// parked `pending_approval` run swept by the TTL expiry). All of
    /// `completed` / `failed` / `cancelled` are terminal.
    pub status: String,
    /// RFC3339 timestamp when the run started.
    pub started_at: String,
    /// RFC3339 timestamp when the run last settled (completed/paused/failed).
    /// `None` while a run row is still `"running"`.
    pub finished_at: Option<String>,
    /// Reconstructed per-node steps (see [`FlowRunStep`]).
    #[serde(default)]
    pub steps: Vec<FlowRunStep>,
    /// Node ids paused awaiting human approval when `status ==
    /// "pending_approval"`; empty otherwise.
    #[serde(default)]
    pub pending_approvals: Vec<String>,
    /// Error message when `status == "failed"`.
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinyflows::model::{Node, NodeKind};

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            nodes: vec![Node {
                id: "t".to_string(),
                kind: NodeKind::Trigger,
                type_version: 1,
                name: "Trigger".to_string(),
                config: serde_json::Value::Null,
                ports: Vec::new(),
                position: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn flow_round_trips_through_json() {
        let flow = Flow {
            id: "flow_1".to_string(),
            name: "demo".to_string(),
            enabled: true,
            graph: sample_graph(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            last_run_at: None,
            last_status: None,
            require_approval: false,
        };
        let json = serde_json::to_string(&flow).expect("serialize");
        let back: Flow = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, flow.id);
        assert_eq!(back.graph, flow.graph);
        assert!(back.last_run_at.is_none());
        assert!(!back.require_approval);
    }

    #[test]
    fn flow_require_approval_defaults_false_when_omitted_from_json() {
        // Legacy/serialized JSON authored before the field existed must still
        // deserialize (SQLite rows are migrated via `add_column_if_missing`,
        // but any bare JSON fixture should also default safely).
        let json = serde_json::json!({
            "id": "flow_1",
            "name": "demo",
            "enabled": true,
            "graph": sample_graph(),
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
        });
        let flow: Flow = serde_json::from_value(json).expect("deserialize");
        assert!(!flow.require_approval);
    }

    #[test]
    fn flow_run_round_trips_through_json() {
        let run = FlowRun {
            id: "flow:flow_1:run-uuid".to_string(),
            flow_id: "flow_1".to_string(),
            thread_id: "flow:flow_1:run-uuid".to_string(),
            status: "completed".to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: Some("2026-01-01T00:00:01Z".to_string()),
            steps: vec![FlowRunStep {
                node_id: "t".to_string(),
                output: serde_json::json!([{"json": {"hello": "world"}}]),
                port: None,
                ..Default::default()
            }],
            pending_approvals: Vec::new(),
            error: None,
        };
        let json = serde_json::to_string(&run).expect("serialize");
        let back: FlowRun = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, run.id);
        assert_eq!(back.steps.len(), 1);
        assert_eq!(back.steps[0].node_id, "t");
        assert!(back.steps[0].port.is_none());
    }

    #[test]
    fn flow_run_step_omits_port_when_none() {
        let step = FlowRunStep {
            node_id: "n".to_string(),
            output: serde_json::Value::Null,
            port: None,
            ..Default::default()
        };
        let v = serde_json::to_value(&step).unwrap();
        assert!(v.get("port").is_none());
    }
}
