//! Runs a registered graph against the checkpointer + event bus, and resumes a
//! paused (human-in-the-loop) run. This is the bridge the RPC layer calls.

use anyhow::anyhow;

use super::build_definition;
use super::state::ProductState;
use crate::openhuman::agent_graph::checkpoint::{Checkpoint, Checkpointer, GraphRunRecord};
use crate::openhuman::agent_graph::graph::{CompiledGraph, GraphCancel};
use crate::openhuman::agent_graph::hitl::ApplyResume;
use crate::openhuman::agent_graph::observability::EventBusSink;
use crate::openhuman::agent_graph::types::{GraphRunStatus, RunResult};
use crate::openhuman::config::Config;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Build a [`GraphRunRecord`] from an engine [`RunResult`].
fn record_from_result(
    run_id: &str,
    graph_name: &str,
    created_at: String,
    result: &RunResult<ProductState>,
) -> Result<GraphRunRecord, String> {
    let state =
        serde_json::to_value(&result.state).map_err(|e| format!("serialize graph state: {e}"))?;
    Ok(GraphRunRecord {
        run_id: run_id.to_string(),
        graph_name: graph_name.to_string(),
        status: result.status,
        created_at,
        updated_at: now(),
        last_node: result.last_node.clone(),
        steps: result.steps,
        state,
        interrupt: result.interrupt.clone(),
        error: None,
        transitions: result.transitions.clone(),
    })
}

/// Persist a run record + a checkpoint snapshot, and emit the terminal bus
/// event.
async fn persist(
    store: &dyn Checkpointer,
    sink: &EventBusSink,
    rec: &GraphRunRecord,
) -> Result<(), String> {
    store.save_run(rec).await.map_err(|e| e.to_string())?;
    let label = match rec.status {
        GraphRunStatus::Paused => rec
            .interrupt
            .as_ref()
            .map(|i| format!("pause:{}", i.kind))
            .unwrap_or_else(|| "pause".to_string()),
        GraphRunStatus::Completed => "complete".to_string(),
        GraphRunStatus::Failed => "failed".to_string(),
        _ => "snapshot".to_string(),
    };
    store
        .save_checkpoint(&Checkpoint {
            id: 0,
            run_id: rec.run_id.clone(),
            step: rec.steps,
            node: rec.last_node.clone(),
            label,
            state: rec.state.clone(),
            created_at: now(),
        })
        .await
        .map_err(|e| e.to_string())?;
    match rec.status {
        GraphRunStatus::Completed => sink.run_completed(&rec.run_id, rec.steps, &rec.last_node),
        GraphRunStatus::Paused => { /* executor already emitted GraphRunPaused */ }
        GraphRunStatus::Failed => {
            sink.run_failed(&rec.run_id, rec.error.as_deref().unwrap_or("unknown"))
        }
        _ => {}
    }
    Ok(())
}

/// Start a new run of the named graph with `input` (an object of seed vars).
pub async fn run_graph(
    config: &Config,
    store: &dyn Checkpointer,
    name: &str,
    input: serde_json::Value,
) -> Result<GraphRunRecord, String> {
    let config_arc = std::sync::Arc::new(config.clone());
    let graph: CompiledGraph<ProductState> = build_definition(name, config_arc)?;
    let run_id = uuid::Uuid::new_v4().to_string();
    let created_at = now();
    let sink = EventBusSink::new(name);
    sink.run_started(&run_id, graph.entry());

    let init = ProductState::from_input(input);
    let cancel = GraphCancel::new();

    tracing::info!(run_id = %run_id, graph = name, "[agent_graph] run_graph start");
    let result = graph
        .invoke_with(init, &run_id, &sink, &cancel)
        .await
        .map_err(|e| {
            sink.run_failed(&run_id, &e.to_string());
            format!("graph run failed: {e}")
        })?;

    let rec = record_from_result(&run_id, name, created_at, &result)?;
    persist(store, &sink, &rec).await?;
    Ok(rec)
}

/// Resume a paused run with the human's `input`.
pub async fn resume_graph(
    config: &Config,
    store: &dyn Checkpointer,
    run_id: &str,
    input: &str,
) -> Result<GraphRunRecord, String> {
    let rec = store
        .load_run(run_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("graph run '{run_id}' not found"))?;
    if rec.status != GraphRunStatus::Paused {
        return Err(format!(
            "graph run '{run_id}' is '{}', not paused — cannot resume",
            rec.status.as_str()
        ));
    }
    let resume_node = rec
        .interrupt
        .as_ref()
        .and_then(|i| i.resume_to.clone())
        .ok_or_else(|| anyhow!("paused run has no resume node").to_string())?;

    let mut state: ProductState =
        serde_json::from_value(rec.state.clone()).map_err(|e| format!("restore state: {e}"))?;
    state.apply_resume(input);

    let config_arc = std::sync::Arc::new(config.clone());
    let graph = build_definition(&rec.graph_name, config_arc)?;
    let sink = EventBusSink::new(&rec.graph_name);
    let cancel = GraphCancel::new();

    tracing::info!(run_id, graph = %rec.graph_name, resume_node = %resume_node, "[agent_graph] resume_graph");
    let result = graph
        .resume_with(state, resume_node, run_id, &sink, &cancel, rec.steps)
        .await
        .map_err(|e| {
            sink.run_failed(run_id, &e.to_string());
            format!("graph resume failed: {e}")
        })?;

    let updated = record_from_result(run_id, &rec.graph_name, rec.created_at.clone(), &result)?;
    persist(store, &sink, &updated).await?;
    Ok(updated)
}
