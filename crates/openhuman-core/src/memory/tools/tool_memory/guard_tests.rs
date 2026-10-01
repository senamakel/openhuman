//! Host-integration tests for the tool-memory agent tools: they run the
//! `tinymemory-tools` implementations against this host's real guarded driver,
//! so the tier gate, the ambient workspace and the shared store are all live.
//! The tools' own argument, schema and wording tests live with them in
//! `tinymemory-tools`.

use super::*;

use crate::memory::api::provider::MemoryProvider;
use crate::memory::api::tool_memory::{ToolMemoryPriority, ToolMemorySource};

use tempfile::TempDir;

use crate::config::test_env::EnvVarGuard;
use crate::config::Config;
use crate::memory::guard::policy::GUARD_DENIED_PREFIX;
use crate::security::live_policy;
use crate::security::policy::{AutonomyLevel, SecurityPolicy};
use serde_json::json;
use std::sync::Arc;
use tinytools::Tool;

/// Install `autonomy` as the live policy for this test thread only. Same
/// shape `memory/guard/policy_tests.rs` uses; `#[tokio::test]`'s
/// current-thread runtime keeps the future on the installing thread.
fn scoped_tier(autonomy: AutonomyLevel) -> live_policy::TestPolicyGuard {
    let dir = std::env::temp_dir();
    live_policy::install_scoped(
        Arc::new(SecurityPolicy {
            autonomy,
            ..SecurityPolicy::default()
        }),
        dir.clone(),
        dir,
    )
}

async fn isolated_config(tmp: &TempDir) -> (EnvVarGuard, Config) {
    let guard = EnvVarGuard::workspace(tmp.path());
    let config = Config::load_or_init().await.expect("load config");
    (guard, config)
}

#[tokio::test]
async fn execute_success_path_persists_rule_in_isolated_workspace() {
    let _serial = crate::memory::ops::GLOBAL_MEMORY_TEST_LOCK.lock().await;
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let tool = MemoryToolsPutTool::default();
    let result = tool
        .execute(json!({
            "tool_name": "bash",
            "rule": "Always dry-run dangerous commands first",
            "priority": "high",
            "tags": ["safety", "shell"]
        }))
        .await
        .expect("valid memory_tools_put request should succeed in isolated workspace");
    assert!(!result.is_error);

    let parsed: serde_json::Value =
        serde_json::from_str(&result.text()).expect("tool result should be json");
    assert_eq!(parsed["tool_name"], "bash");
    assert_eq!(parsed["rule"], "Always dry-run dangerous commands first");
    assert_eq!(parsed["priority"], "high");
    assert_eq!(parsed["source"], "user_explicit");
    assert_eq!(parsed["tags"], json!(["safety", "shell"]));
    assert!(parsed["id"].as_str().is_some());

    let guard = crate::memory::ops::guard::active_memory_guard()
        .await
        .expect("active memory guard");
    let rules = guard
        .as_tool_memory()
        .expect("embedded driver advertises the tool_memory family")
        .tool_rules("bash")
        .await
        .expect("list stored rules");
    let stored = rules
        .iter()
        .find(|rule| rule.rule == "Always dry-run dangerous commands first")
        .expect("stored bash rule should be present");
    assert_eq!(stored.priority, ToolMemoryPriority::High);
    assert_eq!(stored.source, ToolMemorySource::UserExplicit);
    assert_eq!(stored.tags, vec!["safety".to_string(), "shell".to_string()]);
}

#[tokio::test]
async fn execute_defaults_unknown_priority_to_normal() {
    let _serial = crate::memory::ops::GLOBAL_MEMORY_TEST_LOCK.lock().await;
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let tool = MemoryToolsPutTool::default();
    let result = tool
        .execute(json!({
            "tool_name": "bash",
            "rule": "Prefer printf over echo for escapes",
            "priority": "unexpected"
        }))
        .await
        .expect("unknown priority should still succeed");
    assert!(!result.is_error);

    let parsed: serde_json::Value =
        serde_json::from_str(&result.text()).expect("tool result should be json");
    assert_eq!(parsed["priority"], "normal");
}

/// The behavioural discriminator for the re-point: before it, the tool
/// wrote through an undecorated `MemoryClientRef` and no tier check ran, so
/// a `readonly` agent could still pin rules. Through the guard,
/// `admit_write` calls `enforce_write_tier` first.
#[tokio::test]
async fn execute_is_refused_under_the_readonly_tier() {
    let _serial = crate::memory::ops::GLOBAL_MEMORY_TEST_LOCK.lock().await;
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let _tier = scoped_tier(AutonomyLevel::ReadOnly);
    let tool = MemoryToolsPutTool::default();
    let err = tool
        .execute(json!({
            "tool_name": "bash",
            "rule": "readonly agents must not pin rules"
        }))
        .await
        .expect_err("the readonly tier must refuse a tool-memory write");
    let message = err.to_string();
    assert!(
        message.contains(GUARD_DENIED_PREFIX),
        "refusal must be attributable to the guard: {message}"
    );
}

/// The paired positive case: the same call under `full` succeeds, so the
/// test above is proving the tier gate rather than a broken write path.
#[tokio::test]
async fn execute_succeeds_under_the_full_tier() {
    let _serial = crate::memory::ops::GLOBAL_MEMORY_TEST_LOCK.lock().await;
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let _tier = scoped_tier(AutonomyLevel::Full);
    let tool = MemoryToolsPutTool::default();
    let result = tool
        .execute(json!({
            "tool_name": "bash",
            "rule": "full-tier agents may pin rules"
        }))
        .await
        .expect("the full tier must admit a tool-memory write");
    assert!(!result.is_error);
}

/// `memory_tools_put` and `memory_tools_list` must observe each other now
/// that both resolve through the guard rather than through their own
/// `ToolMemoryStore` handles.
#[tokio::test]
async fn guarded_put_and_guarded_list_share_the_store() {
    let _serial = crate::memory::ops::GLOBAL_MEMORY_TEST_LOCK.lock().await;
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let put = MemoryToolsPutTool::default();
    let stored = put
        .execute(json!({
            "tool_name": "web_search",
            "rule": "prefer primary sources",
            "priority": "critical"
        }))
        .await
        .expect("put should succeed");
    let stored: serde_json::Value =
        serde_json::from_str(&stored.text()).expect("put result should be json");
    let stored_id = stored["id"].as_str().expect("stored id").to_string();

    let list = MemoryToolsListTool::default();
    let listed = list
        .execute(json!({ "tool_name": "web_search" }))
        .await
        .expect("list should succeed");
    let listed: serde_json::Value =
        serde_json::from_str(&listed.text()).expect("list result should be json");
    let ids: Vec<&str> = listed
        .as_array()
        .expect("list returns an array")
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert!(
        ids.contains(&stored_id.as_str()),
        "the guarded list must observe the guarded put: {ids:?}"
    );
}

#[tokio::test]
async fn execute_success_path_returns_json_array_for_isolated_workspace() {
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let tool = MemoryToolsListTool::default();
    let result = tool
        .execute(json!({ "tool_name": "bash" }))
        .await
        .expect("valid tool list request should succeed in isolated workspace");
    assert!(!result.is_error);
    let payload = result.text();
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).expect("result should be valid json");
    assert!(
        parsed.is_array(),
        "list tool rules should serialize a JSON array"
    );
}

#[tokio::test]
async fn execute_accepts_other_tool_names_without_rules() {
    let tmp = TempDir::new().expect("tempdir");
    let (_workspace, _cfg) = isolated_config(&tmp).await;
    let tool = MemoryToolsListTool::default();
    let result = tool
        .execute(json!({ "tool_name": "web_search" }))
        .await
        .expect("arbitrary tool names should succeed even when empty");
    assert!(!result.is_error);
}
