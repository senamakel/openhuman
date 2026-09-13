//! Phase-state helpers: the `phase_states` JSON column shape, the dependency
//! walk that picks the next runnable phase, upstream-output threading into a
//! child's prompt, run-summary synthesis, and the shared `persist` write.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use tinyagents_session::run_ledger::{
    upsert_workflow_run, WorkflowRun, WorkflowRunStatus, WorkflowRunUpsert,
};

use super::super::types::{WorkflowDefinition, WorkflowPhase};
use crate::config::Config;

/// Per-phase status stored inside the run's `phase_states` JSON column.
pub(super) const PHASE_PENDING: &str = "pending";
pub(super) const PHASE_RUNNING: &str = "running";
pub(super) const PHASE_COMPLETED: &str = "completed";
pub(super) const PHASE_FAILED: &str = "failed";

/// Initialise `phase_states` to one `pending` entry per phase, preserving
/// declaration order via an object keyed by phase name.
pub(crate) fn init_phase_states(definition: &WorkflowDefinition) -> Value {
    let mut map = serde_json::Map::new();
    for phase in &definition.phases {
        map.insert(
            phase.name.clone(),
            json!({ "status": PHASE_PENDING, "outputs": [] }),
        );
    }
    Value::Object(map)
}

pub(super) fn phase_status<'a>(phase_states: &'a Value, name: &str) -> Option<&'a str> {
    phase_states
        .get(name)
        .and_then(|p| p.get("status"))
        .and_then(Value::as_str)
}

pub(super) fn set_phase_status(
    phase_states: &mut Value,
    name: &str,
    status: &str,
    outputs: Option<Value>,
) {
    if let Some(obj) = phase_states.as_object_mut() {
        let entry = obj
            .entry(name.to_string())
            .or_insert_with(|| json!({ "status": PHASE_PENDING, "outputs": [] }));
        if let Some(entry_obj) = entry.as_object_mut() {
            entry_obj.insert("status".to_string(), json!(status));
            if let Some(out) = outputs {
                entry_obj.insert("outputs".to_string(), out);
            }
        }
    }
}

pub(super) fn set_phase_reason(phase_states: &mut Value, name: &str, reason: &str) {
    if let Some(obj) = phase_states.as_object_mut() {
        if let Some(entry) = obj.get_mut(name).and_then(Value::as_object_mut) {
            entry.insert("reason".to_string(), json!(reason));
        }
    }
}

/// The first phase that is `pending` (or missing) and whose every dependency is
/// `completed`. Definition order breaks ties so the walk is deterministic.
pub(super) fn next_runnable_phase<'a>(
    definition: &'a WorkflowDefinition,
    phase_states: &Value,
) -> Option<&'a WorkflowPhase> {
    definition.phases.iter().find(|phase| {
        let status = phase_status(phase_states, &phase.name).unwrap_or(PHASE_PENDING);
        if status == PHASE_COMPLETED || status == PHASE_RUNNING {
            return false;
        }
        phase
            .depends_on
            .iter()
            .all(|dep| phase_status(phase_states, dep) == Some(PHASE_COMPLETED))
    })
}

pub(super) fn all_phases_completed(definition: &WorkflowDefinition, phase_states: &Value) -> bool {
    definition
        .phases
        .iter()
        .all(|phase| phase_status(phase_states, &phase.name) == Some(PHASE_COMPLETED))
}

/// Collect the outputs of every completed phase this phase depends on, so they
/// can be threaded into the downstream prompt.
pub(super) fn upstream_outputs(
    _definition: &WorkflowDefinition,
    phase: &WorkflowPhase,
    phase_states: &Value,
) -> Vec<Value> {
    let mut out = Vec::new();
    for dep in &phase.depends_on {
        if let Some(outputs) = phase_states
            .get(dep)
            .and_then(|p| p.get("outputs"))
            .and_then(Value::as_array)
        {
            for item in outputs {
                if let Some(text) = item.get("output").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        out.push(json!({ "phase": dep, "output": text }));
                    }
                }
            }
        }
    }
    out
}

/// Build the prompt for one child in a phase: the run input + the phase's
/// description + upstream outputs threaded in as context.
pub(super) fn phase_prompt(
    input: &Value,
    phase: &WorkflowPhase,
    index_in_phase: usize,
    upstream: &[Value],
) -> String {
    let question = input
        .get("question")
        .or_else(|| input.get("input"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| input.to_string());

    let mut prompt = format!(
        "Workflow phase: {}\n{}\n\nInput:\n{}\n",
        phase.name, phase.description, question
    );
    if phase.agent_ids.len() > 1 {
        prompt.push_str(&format!(
            "\n(You are worker #{} in this phase.)\n",
            index_in_phase + 1
        ));
    }
    if !upstream.is_empty() {
        prompt.push_str("\nContext from prior phases:\n");
        for item in upstream {
            if let (Some(p), Some(o)) = (
                item.get("phase").and_then(Value::as_str),
                item.get("output").and_then(Value::as_str),
            ) {
                prompt.push_str(&format!("- [{p}] {o}\n"));
            }
        }
    }
    prompt
}

/// The synthesize phase's combined output becomes the run summary. Falls back
/// to the last completed phase's output if no phase is literally named
/// `synthesize`.
pub(super) fn synthesize_summary(
    definition: &WorkflowDefinition,
    phase_states: &Value,
) -> Option<String> {
    let pick = |name: &str| -> Option<String> {
        let outputs = phase_states
            .get(name)
            .and_then(|p| p.get("outputs"))
            .and_then(Value::as_array)?;
        let joined = outputs
            .iter()
            .filter_map(|o| o.get("output").and_then(Value::as_str))
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        (!joined.trim().is_empty()).then_some(joined)
    };

    if let Some(summary) = pick("synthesize") {
        return Some(summary);
    }
    // Fall back to the last phase in declaration order with non-empty output.
    definition
        .phases
        .iter()
        .rev()
        .find_map(|phase| pick(&phase.name))
}

/// Persist a run-state update. `terminal` controls whether `completed_at` is
/// stamped.
#[allow(clippy::too_many_arguments)]
pub(super) fn persist(
    config: &Config,
    run: &WorkflowRun,
    phase_states: Value,
    child_run_ids: Vec<String>,
    status: WorkflowRunStatus,
    summary: Option<String>,
    terminal: bool,
) -> Result<WorkflowRun> {
    upsert_workflow_run(
        &config.workspace_dir,
        WorkflowRunUpsert {
            id: run.id.clone(),
            definition_id: run.definition_id.clone(),
            parent_thread_id: run.parent_thread_id.clone(),
            input: run.input.clone(),
            phase_states,
            child_run_ids,
            status,
            summary,
            started_at: Some(run.started_at),
            completed_at: terminal.then(chrono::Utc::now),
        },
    )
    .context("persist workflow run state")
}
