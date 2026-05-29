//! Focused Rust E2E coverage for Worker C module ownership.
//!
//! This suite intentionally stays inside the memory / memory_sync / channels /
//! composio / threads slice and drives the real HTTP JSON-RPC router against
//! an isolated workspace. It avoids live network calls.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::http::header::AUTHORIZATION;
use reqwest::StatusCode;
use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};

use openhuman_core::core::auth::{init_rpc_token, CORE_TOKEN_ENV_VAR};
use openhuman_core::core::jsonrpc::build_core_http_router;

const TEST_RPC_TOKEN: &str = "worker-c-modules-e2e-token";

static AUTH_INIT: OnceLock<()> = OnceLock::new();
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarGuard {
    fn set_to_path(key: &'static str, path: &Path) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, path.as_os_str()) };
        Self { key, old }
    }

    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, old }
    }

    fn unset(key: &'static str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::remove_var(key) };
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

struct Harness {
    rpc_base: String,
    _tmp: TempDir,
    _guards: Vec<EnvVarGuard>,
    join: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.join.abort();
    }
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    let mutex = ENV_LOCK.get_or_init(|| Mutex::new(()));
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn ensure_rpc_auth() {
    AUTH_INIT.get_or_init(|| {
        unsafe { std::env::set_var(CORE_TOKEN_ENV_VAR, TEST_RPC_TOKEN) };
        let token_dir = std::env::temp_dir().join("openhuman-worker-c-e2e-auth");
        init_rpc_token(&token_dir).expect("init rpc auth token");
    });
}

fn write_config(openhuman_dir: &Path) {
    std::fs::create_dir_all(openhuman_dir).expect("create .openhuman");
    let cfg = r#"api_url = "http://127.0.0.1:9"
default_model = "worker-c-e2e-model"
default_temperature = 0.2

[secrets]
encrypt = false

[local_ai]
enabled = false

[memory]
provider = "none"
embedding_provider = "none"
embedding_model = "none"
embedding_dimensions = 0

[memory_tree]
embedding_strict = false
"#;
    std::fs::write(openhuman_dir.join("config.toml"), cfg).expect("write config.toml");
    let _: openhuman_core::openhuman::config::Config =
        toml::from_str(cfg).expect("test config must match schema");
}

async fn setup() -> Harness {
    ensure_rpc_auth();

    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    write_config(&home.join(".openhuman"));

    let guards = vec![
        EnvVarGuard::set_to_path("HOME", home),
        EnvVarGuard::unset("OPENHUMAN_WORKSPACE"),
        EnvVarGuard::unset("BACKEND_URL"),
        EnvVarGuard::unset("VITE_BACKEND_URL"),
        EnvVarGuard::unset("OPENHUMAN_API_URL"),
        EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file"),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_STRICT", "false"),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_ENDPOINT", ""),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_MODEL", ""),
    ];

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind rpc listener");
    let addr = listener.local_addr().expect("rpc listener addr");
    let router = build_core_http_router(false);
    let join = tokio::spawn(async move { axum::serve(listener, router).await });

    Harness {
        rpc_base: format!("http://{addr}"),
        _tmp: tmp,
        _guards: guards,
        join,
    }
}

async fn rpc(base: &str, id: i64, method: &str, params: Value) -> Value {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("client");
    let url = format!("{}/rpc", base.trim_end_matches('/'));
    let response = client
        .post(&url)
        .header(AUTHORIZATION, format!("Bearer {TEST_RPC_TOKEN}"))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .send()
        .await
        .unwrap_or_else(|err| panic!("POST {url} {method}: {err}"));
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "HTTP transport should accept {method}"
    );
    response
        .json::<Value>()
        .await
        .unwrap_or_else(|err| panic!("json for {method}: {err}"))
}

fn ok<'a>(value: &'a Value, context: &str) -> &'a Value {
    if let Some(error) = value.get("error") {
        panic!("{context}: unexpected JSON-RPC error: {error}");
    }
    value
        .get("result")
        .unwrap_or_else(|| panic!("{context}: missing result: {value}"))
}

fn payload<'a>(value: &'a Value, context: &str) -> &'a Value {
    let outer = ok(value, context);
    outer
        .get("data")
        .or_else(|| outer.get("result"))
        .unwrap_or(outer)
}

fn error_message<'a>(value: &'a Value, context: &str) -> &'a str {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context}: expected JSON-RPC error with message: {value}"))
}

fn find_status_entry<'a>(entries: &'a [Value], channel: &str, auth_mode: &str) -> &'a Value {
    entries
        .iter()
        .find(|entry| {
            entry.get("channel_id").and_then(Value::as_str) == Some(channel)
                && entry.get("auth_mode").and_then(Value::as_str) == Some(auth_mode)
        })
        .unwrap_or_else(|| panic!("missing status entry for {channel}/{auth_mode}: {entries:?}"))
}

#[tokio::test]
async fn channels_imessage_config_only_connection_reports_status_and_disconnects() {
    let _lock = env_lock();
    let harness = setup().await;

    let described = rpc(
        &harness.rpc_base,
        1,
        "openhuman.channels_describe",
        json!({ "channel": "imessage" }),
    )
    .await;
    assert_eq!(
        payload(&described, "channels_describe")
            .get("id")
            .and_then(Value::as_str),
        Some("imessage")
    );

    let baseline = rpc(
        &harness.rpc_base,
        2,
        "openhuman.channels_status",
        json!({ "channel": "imessage" }),
    )
    .await;
    let baseline_entries = payload(&baseline, "channels_status baseline")
        .as_array()
        .expect("status entries");
    assert_eq!(
        find_status_entry(baseline_entries, "imessage", "managed_dm")
            .get("connected")
            .and_then(Value::as_bool),
        Some(false)
    );

    let connected = rpc(
        &harness.rpc_base,
        3,
        "openhuman.channels_connect",
        json!({
            "channel": "imessage",
            "authMode": "managed_dm",
            "credentials": { "allowed_contacts": "alice@example.com, +15550100" }
        }),
    )
    .await;
    assert_eq!(
        payload(&connected, "channels_connect imessage")
            .get("status")
            .and_then(Value::as_str),
        Some("connected")
    );

    let after_connect = rpc(
        &harness.rpc_base,
        4,
        "openhuman.channels_status",
        json!({ "channel": "imessage" }),
    )
    .await;
    let connected_entries = payload(&after_connect, "channels_status after connect")
        .as_array()
        .expect("status entries");
    assert_eq!(
        find_status_entry(connected_entries, "imessage", "managed_dm")
            .get("connected")
            .and_then(Value::as_bool),
        Some(true),
        "config-only iMessage connection must be visible through channels_status"
    );

    let disconnected = rpc(
        &harness.rpc_base,
        5,
        "openhuman.channels_disconnect",
        json!({
            "channel": "imessage",
            "authMode": "managed_dm",
            "clearMemory": false
        }),
    )
    .await;
    assert_eq!(
        payload(&disconnected, "channels_disconnect imessage")
            .get("disconnected")
            .and_then(Value::as_bool),
        Some(true)
    );

    let after_disconnect = rpc(
        &harness.rpc_base,
        6,
        "openhuman.channels_status",
        json!({ "channel": "imessage" }),
    )
    .await;
    let disconnected_entries = payload(&after_disconnect, "channels_status after disconnect")
        .as_array()
        .expect("status entries");
    assert_eq!(
        find_status_entry(disconnected_entries, "imessage", "managed_dm")
            .get("connected")
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn composio_direct_mode_api_key_and_static_catalogs_round_trip() {
    let _lock = env_lock();
    let harness = setup().await;

    let capabilities = rpc(
        &harness.rpc_base,
        10,
        "openhuman.composio_list_capabilities",
        json!({}),
    )
    .await;
    let capability_rows = payload(&capabilities, "composio_list_capabilities")
        .get("capabilities")
        .and_then(Value::as_array)
        .expect("capabilities array");
    assert!(
        capability_rows.iter().any(|row| {
            row.get("toolkit").and_then(Value::as_str) == Some("gmail")
                || row.get("toolkit").and_then(Value::as_str) == Some("github")
        }),
        "static capability matrix should expose common toolkits: {capability_rows:?}"
    );

    let agent_ready = rpc(
        &harness.rpc_base,
        11,
        "openhuman.composio_list_agent_ready_toolkits",
        json!({}),
    )
    .await;
    let ready_toolkits = payload(&agent_ready, "composio_list_agent_ready_toolkits")
        .get("toolkits")
        .and_then(Value::as_array)
        .expect("agent-ready toolkits");
    assert!(
        ready_toolkits
            .iter()
            .any(|toolkit| toolkit.as_str() == Some("gmail")),
        "gmail should remain in the agent-ready catalog: {ready_toolkits:?}"
    );

    let mode0 = rpc(
        &harness.rpc_base,
        12,
        "openhuman.composio_get_mode",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&mode0, "composio_get_mode initial")
            .get("api_key_set")
            .and_then(Value::as_bool),
        Some(false)
    );

    let set = rpc(
        &harness.rpc_base,
        13,
        "openhuman.composio_set_api_key",
        json!({
            "api_key": "cmp_worker_c_test_key",
            "activate_direct": true
        }),
    )
    .await;
    assert_eq!(
        payload(&set, "composio_set_api_key")
            .get("mode")
            .and_then(Value::as_str),
        Some("direct")
    );

    let mode1 = rpc(
        &harness.rpc_base,
        14,
        "openhuman.composio_get_mode",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&mode1, "composio_get_mode direct")
            .get("api_key_set")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload(&mode1, "composio_get_mode direct")
            .get("mode")
            .and_then(Value::as_str),
        Some("direct")
    );

    let toolkits = rpc(
        &harness.rpc_base,
        15,
        "openhuman.composio_list_toolkits",
        json!({}),
    )
    .await;
    assert!(
        payload(&toolkits, "composio_list_toolkits direct")
            .get("toolkits")
            .and_then(Value::as_array)
            .expect("toolkits array")
            .is_empty(),
        "direct mode should not call the backend tenant allowlist"
    );

    let cleared = rpc(
        &harness.rpc_base,
        16,
        "openhuman.composio_clear_api_key",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&cleared, "composio_clear_api_key")
            .get("mode")
            .and_then(Value::as_str),
        Some("backend")
    );
}

#[tokio::test]
async fn threads_message_lifecycle_is_persisted_and_validated() {
    let _lock = env_lock();
    let harness = setup().await;

    let upsert = rpc(
        &harness.rpc_base,
        20,
        "openhuman.threads_upsert",
        json!({
            "id": "worker-c-thread",
            "title": "Worker C thread",
            "created_at": "2026-05-29T12:00:00Z",
            "labels": ["worker-c", "e2e"]
        }),
    )
    .await;
    assert_eq!(
        payload(&upsert, "threads_upsert")
            .get("id")
            .and_then(Value::as_str),
        Some("worker-c-thread")
    );

    let append = rpc(
        &harness.rpc_base,
        21,
        "openhuman.threads_message_append",
        json!({
            "thread_id": "worker-c-thread",
            "message": {
                "id": "worker-c-message",
                "content": "Persist this Worker C message",
                "type": "text",
                "extraMetadata": { "phase": "initial" },
                "sender": "user",
                "createdAt": "2026-05-29T12:00:01Z"
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&append, "threads_message_append")
            .get("id")
            .and_then(Value::as_str),
        Some("worker-c-message")
    );

    let listed = rpc(
        &harness.rpc_base,
        22,
        "openhuman.threads_messages_list",
        json!({ "thread_id": "worker-c-thread" }),
    )
    .await;
    let messages = payload(&listed, "threads_messages_list")
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages array");
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].get("content").and_then(Value::as_str),
        Some("Persist this Worker C message")
    );

    let updated = rpc(
        &harness.rpc_base,
        23,
        "openhuman.threads_message_update",
        json!({
            "thread_id": "worker-c-thread",
            "message_id": "worker-c-message",
            "extra_metadata": { "phase": "updated", "verified": true }
        }),
    )
    .await;
    assert_eq!(
        payload(&updated, "threads_message_update").pointer("/extraMetadata/verified"),
        Some(&json!(true))
    );

    let missing_list = rpc(
        &harness.rpc_base,
        24,
        "openhuman.threads_messages_list",
        json!({ "thread_id": "missing-thread" }),
    )
    .await;
    assert_eq!(
        payload(&missing_list, "threads_messages_list missing")
            .get("count")
            .and_then(Value::as_u64),
        Some(0),
        "listing a missing thread is a read-only empty result"
    );

    let missing_append = rpc(
        &harness.rpc_base,
        25,
        "openhuman.threads_message_append",
        json!({
            "thread_id": "missing-thread",
            "message": {
                "id": "missing-message",
                "content": "This should not persist",
                "type": "text",
                "extraMetadata": {},
                "sender": "user",
                "createdAt": "2026-05-29T12:00:02Z"
            }
        }),
    )
    .await;
    assert!(
        error_message(&missing_append, "threads_message_append missing").contains("not found"),
        "mutating a missing thread should return a structured JSON-RPC error: {missing_append}"
    );
}

#[tokio::test]
async fn memory_tree_ingest_feeds_memory_sync_status() {
    let _lock = env_lock();
    let harness = setup().await;

    let ingest = rpc(
        &harness.rpc_base,
        30,
        "openhuman.memory_tree_ingest",
        json!({
            "source_kind": "chat",
            "source_id": "slack:worker-c",
            "owner": "worker-c@example.com",
            "tags": ["worker-c", "memory-sync"],
            "payload": {
                "platform": "slack",
                "channel_label": "worker-c",
                "messages": [
                    {
                        "author": "alice@example.com",
                        "text": "Worker C coverage confirms memory sync status after ingest.",
                        "timestamp": 1780000000000_i64,
                        "source_ref": "slack://worker-c/msg-1"
                    }
                ]
            }
        }),
    )
    .await;
    let ingest_payload = payload(&ingest, "memory_tree_ingest");
    assert_eq!(
        ingest_payload.get("source_id").and_then(Value::as_str),
        Some("slack:worker-c")
    );
    assert_eq!(
        ingest_payload.get("chunks_written").and_then(Value::as_u64),
        Some(1)
    );

    let statuses = rpc(
        &harness.rpc_base,
        31,
        "openhuman.memory_sync_status_list",
        json!({}),
    )
    .await;
    let rows = payload(&statuses, "memory_sync_status_list")
        .get("statuses")
        .and_then(Value::as_array)
        .expect("statuses array");
    let slack = rows
        .iter()
        .find(|row| row.get("provider").and_then(Value::as_str) == Some("slack"))
        .unwrap_or_else(|| panic!("expected slack status row after ingest: {rows:?}"));
    assert_eq!(slack.get("chunks_synced").and_then(Value::as_u64), Some(1));
    assert_eq!(
        slack.get("chunks_pending").and_then(Value::as_u64),
        Some(1),
        "inert embeddings leave the fresh chunk pending until an embed sidecar exists"
    );
}
