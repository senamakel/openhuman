//! Runs a registered graph on the **tinyagents** durable engine, persisting the
//! openhuman run-ledger record + checkpoint snapshot for the RPC surface, and
//! resumes a paused (human-in-the-loop) run. This is the bridge the RPC layer
//! calls (issue #4249).
//!
//! Execution runs on `tinyagents` (`run_with_thread` / `resume`); the openhuman
//! [`Checkpointer`] store still backs `agent_graph_run_list`/`_get`/
//! `_checkpoint_list`. Pause/resume within a process session is durable through
//! a shared in-memory tinyagents checkpointer keyed by run id (the resumable
//! thread state + pending nodes the openhuman row cannot reconstruct on its
//! own). A SQLite-backed `tinyagents::Checkpointer` for cross-restart resume is
//! a follow-up.

use std::sync::{Arc, OnceLock};

use tinyagents::harness::ids::ExecutionStatus;
use tinyagents::{Command as TaCommand, GraphExecution, InMemoryCheckpointer};

use super::state::ProductState;
use super::tinyagents_graphs::{build_tinyagents, ProductGraph};
use crate::openhuman::agent_graph::checkpoint::{Checkpoint, Checkpointer, GraphRunRecord};
use crate::openhuman::agent_graph::observability::EventBusSink;
use crate::openhuman::agent_graph::types::{GraphRunStatus, InterruptRequest, NodeTransition};
use crate::openhuman::config::Config;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Process-wide tinyagents checkpointer shared across `run_graph`/`resume_graph`
/// so a paused HITL run can be resumed within the session.
fn ta_checkpointer() -> Arc<InMemoryCheckpointer<ProductState>> {
    static CK: OnceLock<Arc<InMemoryCheckpointer<ProductState>>> = OnceLock::new();
    CK.get_or_init(|| Arc::new(InMemoryCheckpointer::<ProductState>::new()))
        .clone()
}

/// Map a tinyagents [`ExecutionStatus`] to the openhuman run status. An
/// interrupted run is `Paused` (resumable).
fn status_from_execution(exec: &GraphExecution<ProductState>) -> GraphRunStatus {
    match exec.status.status {
        ExecutionStatus::Completed => GraphRunStatus::Completed,
        ExecutionStatus::Interrupted => GraphRunStatus::Paused,
        ExecutionStatus::Running | ExecutionStatus::Pending => GraphRunStatus::Running,
        ExecutionStatus::Failed | ExecutionStatus::Cancelled => GraphRunStatus::Failed,
    }
}

/// Translate a tinyagents [`Interrupt`](tinyagents::Interrupt) payload into the
/// openhuman [`InterruptRequest`] the RPC + UI consume.
fn interrupt_from_execution(exec: &GraphExecution<ProductState>) -> Option<InterruptRequest> {
    let it = exec.interrupts.first()?;
    let p = &it.payload;
    Some(InterruptRequest {
        kind: p
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("approval")
            .to_string(),
        question: p
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("Approve?")
            .to_string(),
        options: p
            .get("options")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        resume_to: p
            .get("resume_to")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| Some(it.node.as_str().to_string())),
    })
}

/// Build observability transitions from the visited-node trace.
fn transitions_from_execution(exec: &GraphExecution<ProductState>) -> Vec<NodeTransition> {
    let ts = now();
    exec.visited
        .iter()
        .enumerate()
        .map(|(i, node)| NodeTransition {
            step: i as u32,
            node: node.as_str().to_string(),
            command: "ran".to_string(),
            elapsed_ms: 0,
            ts: ts.clone(),
        })
        .collect()
}

/// Build a [`GraphRunRecord`] from a tinyagents [`GraphExecution`].
fn record_from_execution(
    run_id: &str,
    graph_name: &str,
    created_at: String,
    exec: &GraphExecution<ProductState>,
) -> Result<GraphRunRecord, String> {
    let state =
        serde_json::to_value(&exec.state).map_err(|e| format!("serialize graph state: {e}"))?;
    let last_node = exec
        .visited
        .last()
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    Ok(GraphRunRecord {
        run_id: run_id.to_string(),
        graph_name: graph_name.to_string(),
        status: status_from_execution(exec),
        created_at,
        updated_at: now(),
        last_node,
        steps: exec.steps as u32,
        state,
        interrupt: interrupt_from_execution(exec),
        error: exec.status.error.clone(),
        transitions: transitions_from_execution(exec),
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
        GraphRunStatus::Paused => {
            let kind = rec
                .interrupt
                .as_ref()
                .map(|i| i.kind.as_str())
                .unwrap_or("approval");
            sink.emit_paused(&rec.run_id, &rec.last_node, kind);
        }
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
    let graph: ProductGraph =
        build_tinyagents(name, config_arc)?.with_checkpointer(ta_checkpointer());
    let run_id = uuid::Uuid::new_v4().to_string();
    let created_at = now();
    let sink = EventBusSink::new(name);
    sink.run_started(&run_id, name);

    let init = ProductState::from_input(input);

    tracing::info!(run_id = %run_id, graph = name, "[agent_graph] run_graph start (tinyagents)");
    let exec = graph
        .run_with_thread(run_id.clone(), init)
        .await
        .map_err(|e| {
            sink.run_failed(&run_id, &e.to_string());
            format!("graph run failed: {e}")
        })?;

    let rec = record_from_execution(&run_id, name, created_at, &exec)?;
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

    let config_arc = std::sync::Arc::new(config.clone());
    let graph: ProductGraph =
        build_tinyagents(&rec.graph_name, config_arc)?.with_checkpointer(ta_checkpointer());
    let sink = EventBusSink::new(&rec.graph_name);

    tracing::info!(run_id, graph = %rec.graph_name, "[agent_graph] resume_graph (tinyagents)");
    let exec = graph
        .resume(
            run_id,
            TaCommand::resume(serde_json::Value::String(input.to_string())),
        )
        .await
        .map_err(|e| {
            sink.run_failed(run_id, &e.to_string());
            format!("graph resume failed: {e}")
        })?;

    let updated = record_from_execution(run_id, &rec.graph_name, rec.created_at.clone(), &exec)?;
    persist(store, &sink, &updated).await?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    // The openhuman run-ledger store (distinct from the tinyagents checkpointer
    // aliased above).
    use crate::openhuman::agent_graph::checkpoint::InMemoryCheckpointer as OhStore;
    use serde_json::json;

    #[tokio::test]
    async fn demo_review_run_pauses_then_resumes_to_completion() {
        let config = Config::default();
        let store = OhStore::new();

        // Start: pauses at the HITL review gate.
        let paused = run_graph(&config, &store, "demo_review", json!({"task": "ship it"}))
            .await
            .expect("run");
        assert_eq!(paused.status, GraphRunStatus::Paused);
        let interrupt = paused.interrupt.as_ref().expect("interrupt");
        assert_eq!(interrupt.kind, "approval");
        assert_eq!(interrupt.resume_to.as_deref(), Some("review"));

        // The run was persisted to the openhuman ledger.
        let loaded = store.load_run(&paused.run_id).await.unwrap().unwrap();
        assert_eq!(loaded.status, GraphRunStatus::Paused);

        // Resume with approval → completion.
        let done = resume_graph(&config, &store, &paused.run_id, "approve")
            .await
            .expect("resume");
        assert_eq!(done.status, GraphRunStatus::Completed);
        // ProductState serializes as `{vars, steps, resume_input}`, so vars live
        // under `state["vars"]`.
        let vars = &done.state["vars"];
        assert_eq!(
            vars.get("review_decision").and_then(|v| v.as_str()),
            Some("approve")
        );
        assert!(vars.get("final").is_some());
    }

    #[tokio::test]
    async fn resume_of_unknown_run_errors() {
        let config = Config::default();
        let store = OhStore::new();
        assert!(resume_graph(&config, &store, "ghost", "approve")
            .await
            .is_err());
    }
}
