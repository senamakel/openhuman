use super::*;
use crate::openhuman::config::Config;
use tempfile::TempDir;
use tinyflows::model::{Node, NodeKind, WorkflowGraph};

fn test_config(tmp: &TempDir) -> Config {
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.workspace_dir).unwrap();
    config
}

fn trigger_graph() -> WorkflowGraph {
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
fn create_get_list_delete_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();
    assert_eq!(flow.name, "demo");
    assert!(flow.enabled);

    let fetched = get_flow(&config, &flow.id).unwrap().expect("flow present");
    assert_eq!(fetched.id, flow.id);
    assert_eq!(fetched.graph, flow.graph);

    let listed = list_flows(&config).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, flow.id);

    remove_flow(&config, &flow.id).unwrap();
    assert!(get_flow(&config, &flow.id).unwrap().is_none());
    assert!(list_flows(&config).unwrap().is_empty());
}

#[test]
fn get_flow_returns_none_for_unknown_id() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    assert!(get_flow(&config, "missing").unwrap().is_none());
}

#[test]
fn remove_flow_errors_when_not_found() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let err = remove_flow(&config, "missing").unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn set_enabled_toggles_and_persists() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();
    assert!(flow.enabled);

    let disabled = set_enabled(&config, &flow.id, false).unwrap();
    assert!(!disabled.enabled);

    let reloaded = get_flow(&config, &flow.id).unwrap().unwrap();
    assert!(!reloaded.enabled);

    let enabled = set_enabled(&config, &flow.id, true).unwrap();
    assert!(enabled.enabled);
}

#[test]
fn update_flow_graph_bumps_updated_at_and_preserves_created_at() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();

    let mut new_graph = trigger_graph();
    new_graph.name = "renamed-graph".to_string();
    let updated =
        update_flow_graph(&config, &flow.id, "renamed".to_string(), new_graph, false).unwrap();

    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.created_at, flow.created_at);
    assert_eq!(updated.graph.name, "renamed-graph");
}

#[test]
fn record_run_sets_last_run_fields() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();
    assert!(flow.last_run_at.is_none());

    record_run(&config, &flow.id, "completed").unwrap();
    let reloaded = get_flow(&config, &flow.id).unwrap().unwrap();
    assert!(reloaded.last_run_at.is_some());
    assert_eq!(reloaded.last_status.as_deref(), Some("completed"));
}

#[test]
fn stored_graph_older_than_current_schema_is_migrated_on_read() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    // Insert a raw, versionless graph row directly (bypassing create_flow's
    // typed path) to simulate a definition persisted by an older crate build.
    let legacy_graph_json = serde_json::json!({
        "name": "legacy",
        "nodes": [{ "id": "t", "kind": "trigger", "name": "Trigger" }],
        "edges": []
    })
    .to_string();

    with_connection(&config, |conn| {
        conn.execute(
            "INSERT INTO flow_definitions
                (id, name, graph_json, enabled, created_at, updated_at, last_run_at, last_status)
             VALUES ('legacy-1', 'legacy', ?1, 1, '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL, NULL)",
            rusqlite::params![legacy_graph_json],
        )?;
        Ok(())
    })
    .unwrap();

    let loaded = get_flow(&config, "legacy-1").unwrap().expect("row present");
    assert_eq!(
        loaded.graph.schema_version,
        tinyflows::model::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(loaded.graph.nodes.len(), 1);
}

#[test]
fn kv_get_set_round_trips_and_is_namespace_scoped() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    assert!(kv_get(&config, "ns1", "k").unwrap().is_none());

    kv_set(&config, "ns1", "k", &serde_json::json!({"v": 1})).unwrap();
    assert_eq!(
        kv_get(&config, "ns1", "k").unwrap(),
        Some(serde_json::json!({"v": 1}))
    );

    // A different namespace does not see ns1's value.
    assert!(kv_get(&config, "ns2", "k").unwrap().is_none());

    // Overwrite.
    kv_set(&config, "ns1", "k", &serde_json::json!(2)).unwrap();
    assert_eq!(
        kv_get(&config, "ns1", "k").unwrap(),
        Some(serde_json::json!(2))
    );
}

// ── require_approval ─────────────────────────────────────────────────────

#[test]
fn create_flow_persists_require_approval() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), true).unwrap();
    assert!(flow.require_approval);

    let reloaded = get_flow(&config, &flow.id).unwrap().unwrap();
    assert!(reloaded.require_approval);
}

#[test]
fn update_flow_graph_can_change_require_approval() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();
    assert!(!flow.require_approval);

    let updated =
        update_flow_graph(&config, &flow.id, flow.name.clone(), trigger_graph(), true).unwrap();
    assert!(updated.require_approval);

    let reloaded = get_flow(&config, &flow.id).unwrap().unwrap();
    assert!(reloaded.require_approval);
}

#[test]
fn legacy_flow_definitions_row_without_require_approval_column_defaults_false() {
    // A row inserted before the `require_approval` column existed (the
    // `add_column_if_missing` ALTER runs on every `with_connection` call, so
    // this simulates a workspace opened once on an older build).
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let legacy_graph_json = serde_json::to_string(&trigger_graph()).unwrap();
    with_connection(&config, |conn| {
        conn.execute(
            "INSERT INTO flow_definitions
                (id, name, graph_json, enabled, created_at, updated_at, last_run_at, last_status)
             VALUES ('legacy-2', 'legacy', ?1, 1, '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL, NULL)",
            rusqlite::params![legacy_graph_json],
        )?;
        Ok(())
    })
    .unwrap();

    let loaded = get_flow(&config, "legacy-2").unwrap().expect("row present");
    assert!(!loaded.require_approval);
}

// ── list_enabled_flows ────────────────────────────────────────────────────

#[test]
fn list_enabled_flows_excludes_disabled() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let enabled_flow = create_flow(&config, "enabled".to_string(), trigger_graph(), false).unwrap();
    let disabled_flow =
        create_flow(&config, "disabled".to_string(), trigger_graph(), false).unwrap();
    set_enabled(&config, &disabled_flow.id, false).unwrap();

    let enabled = list_enabled_flows(&config).unwrap();
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].id, enabled_flow.id);
}

// ── flow_runs CRUD ────────────────────────────────────────────────────────

#[test]
fn flow_run_insert_finish_get_round_trip() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();

    let thread_id = format!("flow:{}:run-1", flow.id);
    insert_flow_run(
        &config,
        &thread_id,
        &flow.id,
        &thread_id,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();

    let running = get_flow_run(&config, &thread_id)
        .unwrap()
        .expect("row present");
    assert_eq!(running.status, "running");
    assert!(running.finished_at.is_none());
    assert!(running.steps.is_empty());

    let steps = vec![FlowRunStep {
        node_id: "t".to_string(),
        output: serde_json::json!([{"json": {"x": 1}}]),
        port: None,
        ..Default::default()
    }];
    finish_flow_run(
        &config,
        &thread_id,
        "completed",
        "2026-01-01T00:00:01Z",
        &steps,
        &[],
        None,
    )
    .unwrap();

    let finished = get_flow_run(&config, &thread_id)
        .unwrap()
        .expect("row present");
    assert_eq!(finished.status, "completed");
    assert_eq!(
        finished.finished_at.as_deref(),
        Some("2026-01-01T00:00:01Z")
    );
    assert_eq!(finished.steps.len(), 1);
    assert_eq!(finished.steps[0].node_id, "t");
    assert!(finished.pending_approvals.is_empty());
    assert!(finished.error.is_none());
}

#[test]
fn finish_flow_run_records_error_on_failure() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();
    let thread_id = format!("flow:{}:run-2", flow.id);
    insert_flow_run(
        &config,
        &thread_id,
        &flow.id,
        &thread_id,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();

    finish_flow_run(
        &config,
        &thread_id,
        "failed",
        "2026-01-01T00:00:01Z",
        &[],
        &[],
        Some("boom"),
    )
    .unwrap();

    let finished = get_flow_run(&config, &thread_id).unwrap().unwrap();
    assert_eq!(finished.status, "failed");
    assert_eq!(finished.error.as_deref(), Some("boom"));
}

#[test]
fn get_flow_run_returns_none_for_unknown_id() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    assert!(get_flow_run(&config, "missing").unwrap().is_none());
}

#[test]
fn list_flow_runs_orders_newest_first_and_is_scoped_to_flow() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow_a = create_flow(&config, "a".to_string(), trigger_graph(), false).unwrap();
    let flow_b = create_flow(&config, "b".to_string(), trigger_graph(), false).unwrap();

    insert_flow_run(
        &config,
        "run-a1",
        &flow_a.id,
        "run-a1",
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    insert_flow_run(
        &config,
        "run-a2",
        &flow_a.id,
        "run-a2",
        "2026-01-02T00:00:00Z",
    )
    .unwrap();
    insert_flow_run(
        &config,
        "run-b1",
        &flow_b.id,
        "run-b1",
        "2026-01-01T00:00:00Z",
    )
    .unwrap();

    let runs_a = list_flow_runs(&config, &flow_a.id, 10).unwrap();
    assert_eq!(runs_a.len(), 2);
    assert_eq!(runs_a[0].id, "run-a2", "newest run must come first");
    assert_eq!(runs_a[1].id, "run-a1");

    let runs_b = list_flow_runs(&config, &flow_b.id, 10).unwrap();
    assert_eq!(runs_b.len(), 1);
    assert_eq!(runs_b[0].id, "run-b1");
}

#[test]
fn list_flow_runs_respects_limit() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let flow = create_flow(&config, "demo".to_string(), trigger_graph(), false).unwrap();

    for i in 0..3 {
        let id = format!("run-{i}");
        insert_flow_run(
            &config,
            &id,
            &flow.id,
            &id,
            &format!("2026-01-0{}T00:00:00Z", i + 1),
        )
        .unwrap();
    }

    let limited = list_flow_runs(&config, &flow.id, 2).unwrap();
    assert_eq!(limited.len(), 2);
}
