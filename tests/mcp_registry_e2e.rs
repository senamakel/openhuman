//! End-to-end test for the `mcp_registry` connection lifecycle.
//!
//! Hermetic: spawns the `test-mcp-stub` binary (built alongside this test
//! by Cargo and exposed via `CARGO_BIN_EXE_test-mcp-stub`) as the MCP
//! subprocess. No npx, no network. Validates that
//! `store::insert_server` → `connections::connect` → `connections::call_tool`
//! → `connections::disconnect` round-trips correctly through the unified
//! `mcp_client::McpStdioClient` transport.

// Exercises the gated `mcp_registry` / `mcp_client` surface, so the whole suite
// is compiled only when the `mcp` feature is on. Without this gate the slim
// build's `cargo test --no-default-features --tests` fails to compile against the removed APIs (#4799).
#![cfg(feature = "mcp")]

use openhuman_core::openhuman::config::Config;
use tinymcp_bus::{CommandKind, InstalledServer, Transport};

/// The service over `config`'s workspace.
///
/// Resolved the same way the RPC handlers resolve it, so a connection this test
/// opens directly is the same one a handler sees. Each case uses its own
/// workspace, so each gets its own store.
fn host(config: &Config) -> std::sync::Arc<openhuman_core::openhuman::mcp::host::McpHost> {
    openhuman_core::openhuman::mcp::host::for_config(config).expect("the mcp host opens")
}

/// Runs exactly one supervision cycle against `host`.
///
/// Driven a tick at a time rather than through the loop, so the test does not
/// wait on a timer for something it can ask for directly.
async fn supervise_once(host: &openhuman_core::openhuman::mcp::host::McpHost) {
    let mut supervisor = tinymcp::Supervisor::new(
        tinymcp::SupervisorConfig::default(),
        tinymcp_bus::McpClientIdentityConfig::default(),
        None,
    );

    supervisor
        .tick(
            host.dynamic().store(),
            host.dynamic().connections(),
            host.dynamic().oauth(),
            std::time::Instant::now(),
        )
        .await;
}

fn fresh_workspace_config() -> (tempfile::TempDir, Config) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut cfg = Config::default();
    cfg.workspace_dir = tmp.path().to_path_buf();
    (tmp, cfg)
}

fn make_installed_server() -> InstalledServer {
    let stub_path = env!("CARGO_BIN_EXE_test-mcp-stub");
    InstalledServer {
        server_id: format!("test-{}", uuid::Uuid::new_v4()),
        qualified_name: "@openhuman-test/echo".to_string(),
        display_name: "Test Echo".to_string(),
        description: Some("Stub MCP server used by mcp_registry_e2e tests.".into()),
        icon_url: None,
        command_kind: CommandKind::Binary,
        command: stub_path.to_string(),
        args: Vec::new(),
        env_keys: Vec::new(),
        config: None,
        installed_at: 0,
        last_connected_at: None,
        transport: Transport::Stdio,
        enabled: true,
    }
}

#[tokio::test]
async fn connect_lists_one_tool_then_disconnect() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let server = make_installed_server();

    // Insert into the store so `all_status` (which reads from store) sees it,
    // and so a follow-up `boot::spawn_installed_servers` would pick it up.
    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");

    // Connect: spawns the stub subprocess and runs `initialize` + `tools/list`.
    let tools = h
        .dynamic()
        .connect(&server.server_id)
        .await
        .expect("connect succeeds")
        .tools;
    assert_eq!(tools.len(), 1, "stub advertises one tool");
    assert_eq!(tools[0].name, "echo");
    assert!(tools[0].input_schema.is_object());

    // Status reflects the live connection.
    let statuses = h.dynamic().status().await.expect("status");
    let mine = statuses
        .iter()
        .find(|s| s.server_id == server.server_id)
        .expect("status entry present");
    assert_eq!(mine.tool_count, 1);

    // Call the `echo` tool and verify the response payload.
    let result = h
        .dynamic()
        .tool_call(
            &server.server_id,
            "echo",
            serde_json::json!({ "message": "hello mcp" }),
        )
        .await
        .expect("call_tool succeeds");

    let text = result
        .result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    assert_eq!(text, "hello mcp", "echo tool returns the input verbatim");

    // Disconnect: removes from the registry and closes the subprocess.
    let removed = h
        .dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
    assert!(removed, "disconnect drops the live connection");

    // Subsequent call fails because the server_id is no longer connected.
    let err = h
        .dynamic()
        .tool_call(
            &server.server_id,
            "echo",
            serde_json::json!({ "message": "post-disconnect" }),
        )
        .await
        .expect_err("call_tool fails after disconnect");
    assert!(err.to_string().contains("not connected"));
}

#[tokio::test]
async fn unknown_tool_call_returns_error() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let server = make_installed_server();

    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");

    h.dynamic()
        .connect(&server.server_id)
        .await
        .expect("connect")
        .tools;

    let err = h
        .dynamic()
        .tool_call(&server.server_id, "does_not_exist", serde_json::json!({}))
        .await
        .expect_err("stub rejects unknown tools");
    assert!(
        err.to_string().to_lowercase().contains("unknown tool")
            || err.to_string().contains("error"),
        "expected unknown-tool error, got: {err}"
    );

    let _ = h
        .dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
}

#[tokio::test]
async fn failed_connect_records_last_error() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.command = "/this/path/does/not/exist".to_string();

    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");

    let err = h
        .dynamic()
        .connect(&server.server_id)
        .await
        .expect_err("connect should fail for bogus command");
    assert!(!err.to_string().is_empty());

    let recorded = h
        .dynamic()
        .connections()
        .last_error(&server.server_id)
        .await;
    assert!(
        recorded.is_some(),
        "LAST_ERRORS must hold the connect failure for server_id={}",
        server.server_id
    );
}

#[tokio::test]
async fn successful_connect_clears_last_error() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();

    // Connecting resolves the install from the store, so a change to the record
    // only takes effect once it is written there.
    server.command = "/nonexistent".to_string();
    h.dynamic().store().insert_server(&server).expect("insert");
    let _ = h.dynamic().connect(&server.server_id).await;
    assert!(h
        .dynamic()
        .connections()
        .last_error(&server.server_id)
        .await
        .is_some());

    server.command = env!("CARGO_BIN_EXE_test-mcp-stub").to_string();
    h.dynamic()
        .store()
        .delete_server(&server.server_id)
        .expect("drop the bogus record");
    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("reinsert");
    h.dynamic()
        .connect(&server.server_id)
        .await
        .expect("real connect succeeds")
        .tools;
    assert!(
        h.dynamic()
            .connections()
            .last_error(&server.server_id)
            .await
            .is_none(),
        "successful connect must clear the prior error"
    );

    let _ = h
        .dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
}

#[tokio::test]
async fn status_priority_disabled_outranks_connected() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.enabled = false;
    h.dynamic().store().insert_server(&server).expect("insert");

    let statuses = h.dynamic().status().await.expect("status");
    let mine = statuses
        .iter()
        .find(|s| s.server_id == server.server_id)
        .expect("status entry present");
    assert_eq!(
        mine.status.as_str(),
        "disabled",
        "disabled server reports `disabled` even before any connect attempt"
    );
    assert!(mine.last_error.is_none());
}

#[tokio::test]
async fn status_reflects_last_connect_error() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.command = "/nonexistent".to_string();
    h.dynamic().store().insert_server(&server).expect("insert");

    let _ = h.dynamic().connect(&server.server_id).await;
    let statuses = h.dynamic().status().await.expect("status");
    let mine = statuses
        .iter()
        .find(|s| s.server_id == server.server_id)
        .unwrap();
    assert_eq!(mine.status.as_str(), "error");
    assert!(
        mine.last_error
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "last_error populated"
    );
}

#[tokio::test]
async fn boot_skips_disabled_servers_and_records_errors() {
    use openhuman_core::openhuman::mcp::registry::boot;

    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);

    // Server A: enabled, real stub → connects.
    let mut a = make_installed_server();
    a.server_id = format!("a-{}", uuid::Uuid::new_v4());
    h.dynamic().store().insert_server(&a).expect("insert a");

    // Server B: enabled but command does not exist → records error, doesn't crash boot.
    let mut b = make_installed_server();
    b.server_id = format!("b-{}", uuid::Uuid::new_v4());
    b.command = "/nonexistent-mcp".to_string();
    h.dynamic().store().insert_server(&b).expect("insert b");

    // Server C: disabled AND command is bogus. If boot ever attempts to
    // connect this server, the bogus command will fail and LAST_ERRORS will
    // hold an entry. The skip is the only way the post-boot last_error stays
    // None — so the assertion below proves the skip actually fired, not just
    // that the Disabled-priority logic masked the failure.
    let mut c = make_installed_server();
    c.server_id = format!("c-{}", uuid::Uuid::new_v4());
    c.enabled = false;
    c.command = "/nonexistent-disabled-server".to_string();
    h.dynamic().store().insert_server(&c).expect("insert c");

    boot::spawn_installed_servers(&cfg).await;

    // A is connected; B recorded an error; C never attempted (no error
    // recorded despite the bogus command).
    let statuses = h.dynamic().status().await.expect("status");
    let by_id = |id: &str| {
        statuses
            .iter()
            .find(|s| s.server_id == id)
            .cloned()
            .unwrap()
    };
    assert_eq!(by_id(&a.server_id).status.as_str(), "connected");
    assert_eq!(by_id(&b.server_id).status.as_str(), "error");
    assert_eq!(by_id(&c.server_id).status.as_str(), "disabled");
    assert!(
        h.dynamic()
            .connections()
            .last_error(&c.server_id)
            .await
            .is_none(),
        "disabled server with bogus command must not have been connect-attempted"
    );

    let _ = h.dynamic().connections().disconnect(&a.server_id).await;
}

#[tokio::test]
async fn set_enabled_false_disconnects_running_server() {
    use openhuman_core::openhuman::mcp::registry::ops;

    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let server = make_installed_server();
    h.dynamic().store().insert_server(&server).expect("insert");
    h.dynamic()
        .connect(&server.server_id)
        .await
        .expect("connect")
        .tools;

    let outcome = ops::mcp_clients_set_enabled(&cfg, server.server_id.clone(), false)
        .await
        .expect("set_enabled ok");
    assert_eq!(outcome.value["enabled"], serde_json::json!(false));

    let loaded = h.dynamic().store().get_server(&server.server_id).unwrap();
    assert!(!loaded.enabled);
    // The `enabled` flag and the `disabled` status string are both derived from
    // the store record, so on their own they would not catch a connection that
    // survived the toggle. Disabling a running server must drop the live
    // connection too.
    assert!(
        !h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await,
        "disabling a running server must drop its live connection"
    );
    let statuses = h.dynamic().status().await.expect("status");
    let mine = statuses
        .iter()
        .find(|s| s.server_id == server.server_id)
        .unwrap();
    assert_eq!(mine.status.as_str(), "disabled");
}

#[tokio::test]
async fn connect_refuses_disabled_server() {
    use openhuman_core::openhuman::mcp::registry::ops;

    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.enabled = false;
    h.dynamic().store().insert_server(&server).expect("insert");

    let err = ops::mcp_clients_connect(&cfg, server.server_id.clone())
        .await
        .expect_err("connect must reject disabled server");
    assert!(
        err.to_string().to_lowercase().contains("disabled"),
        "got: {err}"
    );
}

#[tokio::test]
async fn set_enabled_true_clears_disabled_status_but_does_not_auto_connect() {
    use openhuman_core::openhuman::mcp::registry::ops;

    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.enabled = false;
    h.dynamic().store().insert_server(&server).expect("insert");

    ops::mcp_clients_set_enabled(&cfg, server.server_id.clone(), true)
        .await
        .expect("set_enabled true ok");
    let statuses = h.dynamic().status().await.expect("status");
    let mine = statuses
        .iter()
        .find(|s| s.server_id == server.server_id)
        .unwrap();
    assert_eq!(
        mine.status.as_str(),
        "disconnected",
        "re-enabling alone must not bring up the subprocess; the user calls connect explicitly"
    );
}

#[tokio::test]
async fn update_env_on_disabled_server_persists_but_does_not_reconnect() {
    use openhuman_core::openhuman::mcp::registry::ops;
    use std::collections::HashMap;

    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.enabled = false;
    h.dynamic().store().insert_server(&server).expect("insert");

    let mut env = HashMap::new();
    env.insert("API_KEY".to_string(), "deadbeef".to_string());

    let outcome = ops::mcp_clients_update_env(&cfg, server.server_id.clone(), env)
        .await
        .expect("update_env on disabled server returns Ok");
    assert_eq!(
        outcome.value["status"], "disabled",
        "disabled server reports status=disabled instead of reconnecting"
    );

    let statuses = h.dynamic().status().await.expect("status");
    let mine = statuses
        .iter()
        .find(|s| s.server_id == server.server_id)
        .unwrap();
    assert_eq!(mine.status.as_str(), "disabled");
}

#[tokio::test]
async fn update_env_merges_partial_update_preserving_other_secrets() {
    // Regression for the #3648 review: `update_env` must MERGE a partial
    // payload over the stored env, not replace-all. The connect modal can only
    // send the field the user just typed (it cannot display existing secrets),
    // so a replace-all would silently erase every other stored credential.
    use openhuman_core::openhuman::mcp::registry::ops;
    use std::collections::HashMap;

    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    // Disabled so update_env persists without attempting a live reconnect —
    // we only assert the persisted env here.
    server.enabled = false;
    h.dynamic().store().insert_server(&server).expect("insert");

    // Seed two stored secrets.
    let mut initial = HashMap::new();
    initial.insert("API_KEY".to_string(), "key-1".to_string());
    initial.insert("OTHER_SECRET".to_string(), "other-1".to_string());
    h.dynamic()
        .store()
        .set_env_values(&server.server_id, &initial.clone().into_iter().collect())
        .expect("seed env");

    // Partial update: only API_KEY, as the connect modal would send for a
    // single edited field.
    let mut partial = HashMap::new();
    partial.insert("API_KEY".to_string(), "key-2".to_string());
    ops::mcp_clients_update_env(&cfg, server.server_id.clone(), partial)
        .await
        .expect("update_env returns Ok");

    let stored = h
        .dynamic()
        .store()
        .load_env_values(&server.server_id)
        .expect("load env");
    assert_eq!(
        stored.get("API_KEY").map(String::as_str),
        Some("key-2"),
        "the supplied value must be updated"
    );
    assert_eq!(
        stored.get("OTHER_SECRET").map(String::as_str),
        Some("other-1"),
        "an un-supplied secret must be PRESERVED, not erased by a partial update"
    );
}

// ── Reconnect supervisor (#3312) ───────────────────────────────────────────────

#[tokio::test]
async fn probe_alive_reflects_transport_liveness() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let server = make_installed_server();
    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");

    h.dynamic()
        .connect(&server.server_id)
        .await
        .expect("connect")
        .tools;
    assert!(
        h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await
    );
    assert!(
        h.dynamic()
            .connections()
            .probe_alive(&server.server_id, std::time::Duration::from_secs(8))
            .await
            .is_alive(),
        "a live stub answers the tools/list probe"
    );

    h.dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
    assert!(
        !h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await
    );
    assert!(
        !h.dynamic()
            .connections()
            .probe_alive(&server.server_id, std::time::Duration::from_secs(8))
            .await
            .is_alive(),
        "a disconnected server is not alive"
    );
}

#[tokio::test]
async fn supervisor_reconnects_a_dropped_server() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let server = make_installed_server();
    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");

    // Bring it up, then simulate a silent transport drop by disconnecting while
    // it stays installed + enabled in the store.
    h.dynamic()
        .connect(&server.server_id)
        .await
        .expect("connect")
        .tools;
    h.dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
    assert!(
        !h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await
    );

    // One supervisor tick should notice the enabled-but-disconnected server and
    // reconnect it.
    supervise_once(&h).await;

    assert!(
        h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await,
        "supervisor reconnects a dropped-but-installed server"
    );
    assert!(h
        .dynamic()
        .connections()
        .probe_alive(&server.server_id, std::time::Duration::from_secs(8))
        .await
        .is_alive());

    h.dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
}

#[tokio::test]
async fn supervisor_leaves_a_healthy_connection_intact() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let server = make_installed_server();
    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");
    h.dynamic()
        .connect(&server.server_id)
        .await
        .expect("connect")
        .tools;

    // A tick over a healthy server must keep it connected (probe succeeds → no
    // disconnect/reconnect churn).
    supervise_once(&h).await;
    assert!(
        h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await
    );

    h.dynamic()
        .connections()
        .disconnect(&server.server_id)
        .await;
}

#[tokio::test]
async fn supervisor_skips_a_disabled_server() {
    let (_tmp, cfg) = fresh_workspace_config();
    let h = host(&cfg);
    let mut server = make_installed_server();
    server.enabled = false;
    h.dynamic()
        .store()
        .insert_server(&server)
        .expect("insert installed server");

    // A disabled server must never be connected by the supervisor.
    supervise_once(&h).await;
    assert!(
        !h.dynamic()
            .connections()
            .is_connected(&server.server_id)
            .await,
        "supervisor does not connect disabled servers"
    );
}
