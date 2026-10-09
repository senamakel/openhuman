//! Cron jobs, the flow catalog and flow state on a configured storage
//! backend, end to end through the core's public functions.
//!
//! Its own test binary because it installs a backend into the process-wide
//! storage slot, which would reroute every other suite's stores in a shared
//! process. One test, so nothing in this binary races the slot either.

use std::sync::Arc;

use openhuman_core::config::Config;
use openhuman_core::cron::{self, Schedule};
use openhuman_core::flows;
use openhuman_core::flows::tinyflows::state::FlowState;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_configured_backend_holds_cron_flows_and_flow_state() {
    let workspace = tempfile::tempdir().unwrap();
    let config = Config {
        workspace_dir: workspace.path().to_path_buf(),
        ..Config::default()
    };
    openhuman_core::storage::install(Arc::new(openhuman_core::storage::MemoryStorage::new()));

    // Cron: the store functions run on blocking threads, as the scheduler does.
    let cron_config = config.clone();
    tokio::task::spawn_blocking(move || {
        let job = cron::add_shell_job(
            &cron_config,
            Some("tick".to_string()),
            Schedule::Every { every_ms: 60_000 },
            "echo hi",
        )
        .unwrap();
        assert_eq!(cron::list_jobs(&cron_config).unwrap().len(), 1);
        let now = chrono::Utc::now();
        cron::record_run(&cron_config, &job.id, now, now, "ok", Some("hi"), 3).unwrap();
        assert_eq!(cron::list_runs(&cron_config, &job.id, 10).unwrap().len(), 1);
        cron::remove_job(&cron_config, &job.id).unwrap();
        assert!(cron::list_jobs(&cron_config).unwrap().is_empty());
    })
    .await
    .unwrap();

    // The flow catalog, through the RPC operations.
    let graph = json!({
        "nodes": [
            { "id": "t", "kind": "trigger", "name": "Manual" },
            { "id": "a", "kind": "agent", "name": "Summarize", "config": { "prompt": "hi" } }
        ],
        "edges": [ { "from_node": "t", "to_node": "a" } ]
    });
    let flow = flows::ops::flows_create(&config, "Digest".to_string(), graph, false)
        .await
        .unwrap()
        .value;
    let listed = flows::ops::flows_list(&config).await.unwrap().value;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, flow.id);

    // Flow state resolves to the backend, and its records read back through
    // the catalog's KV (the engine/dedup sharing is unit-tested in core).
    let state_config = config.clone();
    tokio::task::spawn_blocking(move || {
        assert!(matches!(
            FlowState::open(&state_config, "flow:digest"),
            FlowState::Documents(_)
        ));
        flows::kv_set(&state_config, "flow:digest", "seen", &json!(["a"])).unwrap();
        assert_eq!(
            flows::kv_get(&state_config, "flow:digest", "seen").unwrap(),
            Some(json!(["a"]))
        );
    })
    .await
    .unwrap();

    // The delegation graph's checkpointer lives on the backend too.
    let delegation_config = config.clone();
    openhuman_core::agent::orchestration::open_delegation_checkpointer(&delegation_config).unwrap();

    for path in ["cron/jobs.db", "flows/flows.db", "graph_checkpoints.db"] {
        assert!(
            !workspace.path().join(path).exists(),
            "{path} was not written"
        );
    }

    // Without a backend the classic databases are back in use.
    assert!(openhuman_core::storage::clear());
    let fallback = config.clone();
    tokio::task::spawn_blocking(move || {
        assert!(matches!(
            FlowState::open(&fallback, "flow:digest"),
            FlowState::Sqlite(_)
        ));
    })
    .await
    .unwrap();
    // The flow written to the backend is not in the classic catalog.
    assert!(flows::ops::flows_list(&config)
        .await
        .unwrap()
        .value
        .is_empty());
    openhuman_core::agent::orchestration::open_delegation_checkpointer(&config).unwrap();
    assert!(workspace.path().join("graph_checkpoints.db").exists());
}
