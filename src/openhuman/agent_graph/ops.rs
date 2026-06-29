//! RPC logic for the `agent_graph` domain — thin handlers over the checkpointer
//! + definition registry + runner. All business logic stays here (not in the
//! transport-facing [`schemas`](super::schemas)).

use serde::{Deserialize, Serialize};

use crate::openhuman::agent_graph::blueprint::GraphBlueprint;
use crate::openhuman::agent_graph::checkpoint::{
    Checkpoint, Checkpointer, GraphRunRecord, SqliteCheckpointer,
};
use crate::openhuman::agent_graph::definitions::{self, runner, GraphDefinitionMeta};
use crate::openhuman::config::Config;
use crate::rpc::RpcOutcome;

/// Default page size for `run_list`.
const DEFAULT_LIST_LIMIT: usize = 50;

/// A built-in agent's id paired with its LangGraph-compatible chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGraphEntry {
    /// Agent id.
    pub agent_id: String,
    /// The agent's execution-chain blueprint (from its `graph.rs`).
    pub blueprint: GraphBlueprint,
}

/// A run plus its checkpoints (for `run_get`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRunDetail {
    /// The run record (state, status, interrupt, transitions).
    pub run: GraphRunRecord,
    /// All checkpoints for the run, oldest-first.
    pub checkpoints: Vec<Checkpoint>,
}

fn store(config: &Config) -> SqliteCheckpointer {
    SqliteCheckpointer::new(config.workspace_dir.clone())
}

/// List the registered graph definitions.
pub fn definition_list() -> Result<RpcOutcome<Vec<GraphDefinitionMeta>>, String> {
    let defs = definitions::list_definitions();
    tracing::debug!(count = defs.len(), "[rpc] agent_graph_definition_list");
    Ok(RpcOutcome::single_log(
        defs,
        "agent_graph definitions listed",
    ))
}

/// List every built-in agent's execution-chain blueprint (from its `graph.rs`).
pub fn agent_list() -> Result<RpcOutcome<Vec<AgentGraphEntry>>, String> {
    let entries: Vec<AgentGraphEntry> =
        crate::openhuman::agent_registry::agents::all_builtin_graphs()
            .into_iter()
            .map(|(agent_id, blueprint)| AgentGraphEntry {
                agent_id: agent_id.to_string(),
                blueprint,
            })
            .collect();
    tracing::debug!(count = entries.len(), "[rpc] agent_graph_agent_list");
    Ok(RpcOutcome::single_log(entries, "agent graphs listed"))
}

/// Fetch one built-in agent's chain blueprint by id.
pub fn agent_graph(agent_id: &str) -> Result<RpcOutcome<GraphBlueprint>, String> {
    let bp = crate::openhuman::agent_registry::agents::builtin_graph(agent_id)
        .ok_or_else(|| format!("no built-in agent '{agent_id}'"))?;
    tracing::debug!(agent_id, "[rpc] agent_graph_agent_graph");
    Ok(RpcOutcome::single_log(bp, "agent graph fetched"))
}

/// Start a new run of a named graph with `input` seed vars.
pub async fn run(
    config: &Config,
    name: &str,
    input: serde_json::Value,
) -> Result<RpcOutcome<GraphRunRecord>, String> {
    tracing::info!(graph = name, "[rpc] agent_graph_run");
    let rec = runner::run_graph(config, &store(config), name, input).await?;
    Ok(RpcOutcome::single_log(
        rec,
        format!("agent_graph run '{name}' executed"),
    ))
}

/// List runs, newest-first, paged.
pub async fn run_list(
    config: &Config,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<RpcOutcome<Vec<GraphRunRecord>>, String> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, 500);
    let offset = offset.unwrap_or(0);
    let runs = store(config)
        .list_runs(limit, offset)
        .await
        .map_err(|e| e.to_string())?;
    tracing::debug!(count = runs.len(), "[rpc] agent_graph_run_list");
    Ok(RpcOutcome::single_log(runs, "agent_graph runs listed"))
}

/// Fetch one run with its checkpoints.
pub async fn run_get(config: &Config, run_id: &str) -> Result<RpcOutcome<GraphRunDetail>, String> {
    let s = store(config);
    let run = s
        .load_run(run_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("graph run '{run_id}' not found"))?;
    let checkpoints = s
        .list_checkpoints(run_id)
        .await
        .map_err(|e| e.to_string())?;
    tracing::debug!(
        run_id,
        checkpoints = checkpoints.len(),
        "[rpc] agent_graph_run_get"
    );
    Ok(RpcOutcome::single_log(
        GraphRunDetail { run, checkpoints },
        "agent_graph run fetched",
    ))
}

/// List a run's checkpoints.
pub async fn checkpoint_list(
    config: &Config,
    run_id: &str,
) -> Result<RpcOutcome<Vec<Checkpoint>>, String> {
    let checkpoints = store(config)
        .list_checkpoints(run_id)
        .await
        .map_err(|e| e.to_string())?;
    tracing::debug!(
        run_id,
        count = checkpoints.len(),
        "[rpc] agent_graph_checkpoint_list"
    );
    Ok(RpcOutcome::single_log(
        checkpoints,
        "agent_graph checkpoints listed",
    ))
}

/// Resume a paused (human-in-the-loop) run with the human's input.
pub async fn resume(
    config: &Config,
    run_id: &str,
    input: &str,
) -> Result<RpcOutcome<GraphRunRecord>, String> {
    tracing::info!(run_id, "[rpc] agent_graph_resume");
    let rec = runner::resume_graph(config, &store(config), run_id, input).await?;
    Ok(RpcOutcome::single_log(rec, "agent_graph run resumed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_config() -> Config {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        // Leak the tempdir so the path stays valid for the test's duration.
        std::mem::forget(dir);
        config
    }

    #[test]
    fn definition_list_includes_canonical_turn() {
        let out = definition_list().unwrap();
        let json = out.into_cli_compatible_json().unwrap();
        let s = json.to_string();
        assert!(s.contains("canonical_turn"));
        assert!(s.contains("plan_execute_review"));
    }

    #[tokio::test]
    async fn run_demo_then_list_and_get() {
        let config = test_config();
        // auto_approve so it completes without pausing.
        let started = run(
            &config,
            "demo_review",
            json!({"task": "ship", "auto_approve": true}),
        )
        .await
        .unwrap();
        let rec = started.value.clone();
        assert_eq!(rec.graph_name, "demo_review");
        assert_eq!(
            rec.status,
            crate::openhuman::agent_graph::types::GraphRunStatus::Completed
        );

        let runs = run_list(&config, None, None).await.unwrap();
        assert_eq!(runs.value.len(), 1);

        let detail = run_get(&config, &rec.run_id).await.unwrap();
        assert_eq!(detail.value.run.run_id, rec.run_id);
        assert!(!detail.value.checkpoints.is_empty());
    }

    #[tokio::test]
    async fn run_get_unknown_errors() {
        let config = test_config();
        assert!(run_get(&config, "nope").await.is_err());
    }

    #[tokio::test]
    async fn pause_then_resume_completes() {
        let config = test_config();
        let started = run(&config, "demo_review", json!({"task": "ship"}))
            .await
            .unwrap();
        let rec = started.value.clone();
        assert_eq!(
            rec.status,
            crate::openhuman::agent_graph::types::GraphRunStatus::Paused
        );
        let resumed = resume(&config, &rec.run_id, "approve").await.unwrap();
        assert_eq!(
            resumed.value.status,
            crate::openhuman::agent_graph::types::GraphRunStatus::Completed
        );
    }

    #[tokio::test]
    async fn resume_non_paused_errors() {
        let config = test_config();
        let started = run(
            &config,
            "demo_review",
            json!({"task": "ship", "auto_approve": true}),
        )
        .await
        .unwrap();
        let rec = started.value.clone();
        // Already completed → resume should reject.
        assert!(resume(&config, &rec.run_id, "approve").await.is_err());
    }

    #[tokio::test]
    async fn run_unknown_definition_errors() {
        let config = test_config();
        assert!(run(&config, "ghost", json!({})).await.is_err());
    }
}
