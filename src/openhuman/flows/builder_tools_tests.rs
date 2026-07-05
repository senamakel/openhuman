use super::*;
use crate::openhuman::config::Config;
use crate::openhuman::security::{AutonomyLevel, SecurityPolicy};
use serde_json::json;
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> Arc<Config> {
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.workspace_dir).unwrap();
    Arc::new(config)
}

fn policy(level: AutonomyLevel) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: level,
        ..SecurityPolicy::default()
    })
}

fn valid_graph() -> Value {
    json!({
        "nodes": [
            { "id": "t", "kind": "trigger", "name": "Manual" },
            { "id": "a", "kind": "agent", "name": "Summarize", "config": { "prompt": "hi" } }
        ],
        "edges": [ { "from_node": "t", "to_node": "a" } ]
    })
}

// ── revise_workflow ──────────────────────────────────────────────────────────

#[tokio::test]
async fn revise_workflow_validates_and_returns_revision_proposal() {
    let tmp = TempDir::new().unwrap();
    let tool = ReviseWorkflowTool::new(test_config(&tmp));

    let result = tool
        .execute(json!({
            "name": "Revised flow",
            "graph": valid_graph(),
            "instruction": "add a summarize step"
        }))
        .await
        .unwrap();

    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    assert_eq!(parsed["type"], "workflow_proposal");
    assert_eq!(parsed["revision"], true);
    assert_eq!(parsed["name"], "Revised flow");
    assert_eq!(parsed["instruction"], "add a summarize step");
    assert_eq!(parsed["graph"]["nodes"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn revise_workflow_rejects_invalid_graph() {
    let tmp = TempDir::new().unwrap();
    let tool = ReviseWorkflowTool::new(test_config(&tmp));

    let result = tool
        .execute(json!({
            "name": "bad",
            "graph": { "nodes": [ { "id": "a", "kind": "agent", "name": "A" } ], "edges": [] }
        }))
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.output().to_lowercase().contains("invalid"));
}

#[test]
fn revise_workflow_never_persists() {
    // The revise tool shares propose_workflow's human-in-the-loop invariant:
    // no side effect, no permission gate — it only validates and returns.
    let tmp = TempDir::new().unwrap();
    let tool = ReviseWorkflowTool::new(test_config(&tmp));
    assert_eq!(tool.name(), "revise_workflow");
    assert_eq!(tool.permission_level(), PermissionLevel::None);
    assert!(!tool.external_effect());
}

// ── read-only tools ──────────────────────────────────────────────────────────

#[tokio::test]
async fn list_flows_is_read_only_and_lists() {
    let tmp = TempDir::new().unwrap();
    let tool = ListFlowsTool::new(test_config(&tmp));
    assert_eq!(tool.permission_level(), PermissionLevel::None);
    assert!(!tool.external_effect());

    let result = tool.execute(json!({})).await.unwrap();
    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    // No flows saved in a fresh workspace.
    assert!(parsed["flows"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_flow_missing_id_is_error() {
    let tmp = TempDir::new().unwrap();
    let tool = GetFlowTool::new(test_config(&tmp));
    assert_eq!(tool.permission_level(), PermissionLevel::None);

    let result = tool.execute(json!({})).await.unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("Missing 'id'"));
}

#[tokio::test]
async fn get_flow_unknown_id_is_error() {
    let tmp = TempDir::new().unwrap();
    let tool = GetFlowTool::new(test_config(&tmp));

    let result = tool.execute(json!({ "id": "nope" })).await.unwrap();
    assert!(result.is_error);
    assert!(
        result.output().to_lowercase().contains("not found") || result.output().contains("nope")
    );
}

#[tokio::test]
async fn get_flow_run_missing_id_is_error() {
    let tmp = TempDir::new().unwrap();
    let tool = GetFlowRunTool::new(test_config(&tmp));
    assert_eq!(tool.permission_level(), PermissionLevel::None);

    let result = tool.execute(json!({})).await.unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("Missing 'run_id'"));
}

#[tokio::test]
async fn list_flow_connections_is_read_only() {
    let tmp = TempDir::new().unwrap();
    let tool = ListFlowConnectionsTool::new(test_config(&tmp));
    assert_eq!(tool.permission_level(), PermissionLevel::None);
    assert!(!tool.external_effect());

    let result = tool.execute(json!({})).await.unwrap();
    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    assert!(parsed["connections"].is_array());
}

// ── search_tool_catalog ──────────────────────────────────────────────────────

#[test]
fn search_curated_catalog_finds_real_gmail_slug() {
    // Grounded search over the curated catalog returns a real slug/scope.
    let results = search_curated_catalog("gmail", Some("gmail"), 40);
    assert!(!results.is_empty(), "gmail catalog should have entries");
    for r in &results {
        assert_eq!(r["toolkit"], "gmail");
        assert!(r["slug"]
            .as_str()
            .unwrap()
            .to_ascii_uppercase()
            .starts_with("GMAIL"));
        assert!(r["scope"].is_string());
    }
}

#[test]
fn search_curated_catalog_all_terms_must_match() {
    // A nonsense term matches nothing.
    let results = search_curated_catalog("zzz_no_such_slug_zzz", None, 40);
    assert!(results.is_empty());
}

#[tokio::test]
async fn search_tool_catalog_tool_is_read_only_and_grounds() {
    let tool = SearchToolCatalogTool::new();
    assert_eq!(tool.name(), "search_tool_catalog");
    assert_eq!(tool.permission_level(), PermissionLevel::None);
    assert!(!tool.external_effect());

    let result = tool
        .execute(json!({ "query": "send", "toolkit": "gmail" }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    assert!(parsed["count"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn search_tool_catalog_missing_query_is_error() {
    let tool = SearchToolCatalogTool::new();
    let result = tool.execute(json!({})).await.unwrap();
    assert!(result.is_error);
    assert!(result.output().contains("Missing 'query'"));
}

// ── dry_run_workflow ─────────────────────────────────────────────────────────

#[test]
fn dry_run_is_execute_permission() {
    let tool = DryRunWorkflowTool::new(
        policy(AutonomyLevel::Supervised),
        test_config(&TempDir::new().unwrap()),
    );
    assert_eq!(tool.name(), "dry_run_workflow");
    assert_eq!(tool.permission_level(), PermissionLevel::Execute);
    // Mock-backed: no real outbound effect.
    assert!(!tool.external_effect());
}

#[tokio::test]
async fn dry_run_refused_under_readonly_tier() {
    let tool = DryRunWorkflowTool::new(
        policy(AutonomyLevel::ReadOnly),
        test_config(&TempDir::new().unwrap()),
    );
    let result = tool
        .execute(json!({ "graph": valid_graph() }))
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(result.output().to_lowercase().contains("read-only"));
}

#[tokio::test]
async fn dry_run_supervised_runs_against_mock_and_labels_sandbox() {
    let tool = DryRunWorkflowTool::new(
        policy(AutonomyLevel::Supervised),
        test_config(&TempDir::new().unwrap()),
    );
    let result = tool
        .execute(json!({ "graph": valid_graph(), "input": { "x": 1 } }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    assert_eq!(parsed["sandbox"], true);
    assert_eq!(parsed["ok"], true);
    assert!(parsed["note"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("sandbox"));
}

#[tokio::test]
async fn dry_run_invalid_graph_is_error() {
    let tool = DryRunWorkflowTool::new(
        policy(AutonomyLevel::Full),
        test_config(&TempDir::new().unwrap()),
    );
    let result = tool
        .execute(json!({ "graph": { "nodes": [], "edges": [] } }))
        .await
        .unwrap();
    assert!(result.is_error);
}

#[tokio::test]
async fn dry_run_catches_unwired_required_composio_arg() {
    // Seed the preflight schema cache so no live Composio backend is needed.
    // NOTE: the cache is process-global and other tests seed the `gmail`
    // toolkit too — keep every seeding of GMAIL_SEND_EMAIL identical
    // (`to` + `body`) so test order can't change the outcome.
    let mut entries = std::collections::HashMap::new();
    entries.insert(
        "GMAIL_SEND_EMAIL".to_string(),
        vec!["to".to_string(), "body".to_string()],
    );
    crate::openhuman::tinyflows::caps::seed_required_args_cache("gmail", entries);

    let tmp = TempDir::new().unwrap();
    let tool = DryRunWorkflowTool::new(policy(AutonomyLevel::Supervised), test_config(&tmp));

    let graph_with = |args: Value| {
        json!({
            "nodes": [
                { "id": "t", "kind": "trigger", "name": "Manual" },
                { "id": "send", "kind": "tool_call", "name": "Send email",
                  "config": { "slug": "GMAIL_SEND_EMAIL", "args": args } }
            ],
            "edges": [ { "from_node": "t", "to_node": "send" } ]
        })
    };

    // `to` is a `=`-expression that misses (trigger input has no `email`):
    // the dry run must fail BEFORE the (mock) tool call, naming the field.
    let result = tool
        .execute(json!({
            "graph": graph_with(json!({ "to": "=item.email", "body": "hello" })),
            "input": {}
        }))
        .await
        .unwrap();
    let out = result.output();
    assert!(
        out.contains("`to`") && out.contains("required"),
        "dry run must name the unwired required arg: {out}"
    );

    // The same flow with `to` wired from the trigger passes the preflight.
    let result = tool
        .execute(json!({
            "graph": graph_with(json!({ "to": "=item.email", "body": "hello" })),
            "input": { "email": "a@b.com" }
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    assert_eq!(parsed["sandbox"], true);
    assert_eq!(
        parsed["ok"], true,
        "wired flow must dry-run green: {parsed}"
    );
}

#[tokio::test]
async fn revise_workflow_warns_on_unwired_required_composio_arg() {
    let mut entries = std::collections::HashMap::new();
    entries.insert(
        "GMAIL_SEND_EMAIL".to_string(),
        vec!["to".to_string(), "body".to_string()],
    );
    crate::openhuman::tinyflows::caps::seed_required_args_cache("gmail", entries);

    let tmp = TempDir::new().unwrap();
    let tool = ReviseWorkflowTool::new(test_config(&tmp));
    let result = tool
        .execute(json!({
            "name": "Send mail",
            "graph": {
                "nodes": [
                    { "id": "t", "kind": "trigger", "name": "Manual" },
                    { "id": "send", "kind": "tool_call", "name": "Send",
                      // `body` wired via expression (counts as wired); `to` absent.
                      "config": { "slug": "GMAIL_SEND_EMAIL",
                                  "args": { "body": "=item.text" } } }
                ],
                "edges": [ { "from_node": "t", "to_node": "send" } ]
            }
        }))
        .await
        .unwrap();

    assert!(!result.is_error, "{}", result.output());
    let parsed: Value = serde_json::from_str(&result.output()).unwrap();
    let warnings = parsed["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| {
            let w = w.as_str().unwrap_or_default();
            w.contains("`to`") && w.contains("send")
        }),
        "expected a warning naming node `send` and arg `to`: {warnings:?}"
    );
    // `body` is wired (expression) — no warning for it.
    assert!(
        !warnings
            .iter()
            .any(|w| w.as_str().unwrap_or_default().contains("`body`")),
        "wired arg must not warn: {warnings:?}"
    );
}
