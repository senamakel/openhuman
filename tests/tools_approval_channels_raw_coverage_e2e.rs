//! Raw-line oriented integration coverage for tools, approval, channels, and
//! tool_registry surfaces that are not covered by the narrower controller tests.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::Request;
use axum::http::{header::AUTHORIZATION, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use reqwest::StatusCode as ReqwestStatusCode;
use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};

use openhuman_core::core::auth::{init_rpc_token, CORE_TOKEN_ENV_VAR};
use openhuman_core::core::jsonrpc::build_core_http_router;
use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::credentials::{
    AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME,
};
use openhuman_core::openhuman::memory::{
    Memory, MemoryCategory, MemoryEntry, NamespaceSummary, RecallOpts,
};
use openhuman_core::openhuman::security::{AuditLogger, SecurityPolicy};
use openhuman_core::openhuman::tool_registry::{
    all_tool_registry_controller_schemas, all_tool_registry_registered_controllers,
    registry_entries,
};
use openhuman_core::openhuman::tools::local_cli::tools_wrappers_list_json;
use openhuman_core::openhuman::tools::{
    all_tools, all_tools_controller_schemas, all_tools_registered_controllers, default_tools,
    CleaningStrategy, DefaultToolPolicy, PermissionLevel, PolicyDecision, SchemaCleanr, ToolPolicy,
    ToolScope,
};

const TEST_RPC_TOKEN: &str = "tools-approval-channels-raw-e2e-token";

static AUTH_INIT: OnceLock<()> = OnceLock::new();
static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, old }
    }

    fn set_to_path(key: &'static str, path: &Path) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, path.as_os_str());
        Self { key, old }
    }

    fn unset(key: &'static str) -> Self {
        let old = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct Harness {
    _tmp: TempDir,
    _guards: Vec<EnvVarGuard>,
    rpc_base: String,
    rpc_join: tokio::task::JoinHandle<Result<(), std::io::Error>>,
    backend_join: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

#[derive(Default)]
struct StubMemory;

#[async_trait]
impl Memory for StubMemory {
    fn name(&self) -> &str {
        "stub"
    }

    async fn store(
        &self,
        _namespace: &str,
        _key: &str,
        _content: &str,
        _category: MemoryCategory,
        _session_id: Option<&str>,
    ) -> Result<()> {
        Ok(())
    }

    async fn recall(
        &self,
        _query: &str,
        _limit: usize,
        _opts: RecallOpts<'_>,
    ) -> Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    async fn get(&self, _namespace: &str, _key: &str) -> Result<Option<MemoryEntry>> {
        Ok(None)
    }

    async fn list(
        &self,
        _namespace: Option<&str>,
        _category: Option<&MemoryCategory>,
        _session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    async fn forget(&self, _namespace: &str, _key: &str) -> Result<bool> {
        Ok(false)
    }

    async fn namespace_summaries(&self) -> Result<Vec<NamespaceSummary>> {
        Ok(Vec::new())
    }

    async fn count(&self) -> Result<usize> {
        Ok(0)
    }

    async fn health_check(&self) -> bool {
        true
    }
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ensure_rpc_auth() {
    AUTH_INIT.get_or_init(|| {
        std::env::set_var(CORE_TOKEN_ENV_VAR, TEST_RPC_TOKEN);
        let token_dir = std::env::temp_dir().join("openhuman-tools-channels-e2e-auth");
        init_rpc_token(&token_dir).expect("init rpc auth token");
    });
}

async fn serve_rpc() -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    ensure_rpc_auth();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind rpc listener");
    let addr = listener.local_addr().expect("rpc listener addr");
    let router = build_core_http_router(false);
    let join = tokio::spawn(async move { axum::serve(listener, router).await });
    (addr, join)
}

async fn serve_backend() -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind backend listener");
    let addr = listener.local_addr().expect("backend listener addr");
    let router = Router::new().route("/{*path}", any(mock_backend));
    let join = tokio::spawn(async move { axum::serve(listener, router).await });
    (addr, join)
}

async fn mock_backend(request: Request) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_string();
    let query = uri.query().unwrap_or_default().to_string();
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|_| Bytes::new());
    let json_body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or_else(|_| Value::Null)
    };

    let payload = match (method, path.as_str()) {
        (Method::GET, "/auth/me") => json!({
            "success": true,
            "user": {
                "id": "user-e2e",
                "telegramId": "telegram-user-1",
                "discord_id": "discord-user-1"
            }
        }),
        (Method::POST, "/auth/channels/telegram/link-token") => {
            json!({ "success": true, "data": { "linkToken": "telegram-link-e2e" } })
        }
        (Method::POST, "/auth/channels/discord/link-token") => {
            json!({ "success": true, "data": { "token": "discord-link-e2e" } })
        }
        (Method::POST, "/channels/telegram/messages") => json!({
            "success": true,
            "data": { "messageId": "msg-1", "channel": "telegram", "echo": json_body }
        }),
        (Method::POST, "/channels/telegram/reactions") => json!({
            "success": true,
            "data": { "ok": true, "reaction": json_body }
        }),
        (Method::POST, "/channels/telegram/threads") => json!({
            "success": true,
            "data": { "threadId": "thread-1", "title": json_body.get("title").cloned().unwrap_or(Value::Null) }
        }),
        (Method::PATCH, "/channels/telegram/threads/thread-1") => json!({
            "success": true,
            "data": { "threadId": "thread-1", "action": json_body.get("action").cloned().unwrap_or(Value::Null) }
        }),
        (Method::GET, "/channels/telegram/threads") => json!({
            "success": true,
            "data": {
                "query": query,
                "threads": [{ "threadId": "thread-1", "active": true }]
            }
        }),
        _ => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(json!({ "success": false, "error": format!("unhandled {path}") })),
            )
                .into_response();
        }
    };

    (StatusCode::OK, axum::Json(payload)).into_response()
}

fn write_config(openhuman_dir: &Path, api_url: &str) {
    std::fs::create_dir_all(openhuman_dir).expect("create config dir");
    let cfg = format!(
        r#"api_url = "{api_url}"
default_model = "e2e-model"

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

[autonomy]
level = "full"
workspace_only = false
max_actions_per_hour = 50
require_approval_for_medium_risk = false
block_high_risk_commands = false
auto_approve = []

[node]
enabled = false

[gitbooks]
enabled = false

[mcp_client]
enabled = true

[[mcp_client.servers]]
name = "filesystem"
command = "node"
args = ["server.js"]
enabled = true
allowed_tools = ["read_file"]
disallowed_tools = ["write_file"]
"#
    );
    std::fs::write(openhuman_dir.join("config.toml"), cfg).expect("write config.toml");
}

async fn setup() -> Harness {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path();
    let workspace = home.join("openhuman-workspace");
    let (backend_addr, backend_join) = serve_backend().await;
    let api_url = format!("http://{backend_addr}");

    write_config(&workspace, &api_url);
    write_config(&home.join(".openhuman"), &api_url);

    let guards = vec![
        EnvVarGuard::set_to_path("HOME", home),
        EnvVarGuard::set_to_path("OPENHUMAN_WORKSPACE", &workspace),
        EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file"),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_STRICT", "false"),
        EnvVarGuard::set("OPENHUMAN_TELEGRAM_BOT_USERNAME", "coverage_bot"),
        EnvVarGuard::unset("BACKEND_URL"),
        EnvVarGuard::unset("VITE_BACKEND_URL"),
        EnvVarGuard::unset("OPENHUMAN_API_URL"),
        EnvVarGuard::unset("OPENHUMAN_LSP_ENABLED"),
    ];

    let config = Config::load_or_init()
        .await
        .expect("load config for app session seed");
    let auth = AuthService::from_config(&config);
    auth.store_provider_token(
        APP_SESSION_PROVIDER,
        DEFAULT_AUTH_PROFILE_NAME,
        "header.payload.local",
        HashMap::from([("user_id".to_string(), "user-e2e".to_string())]),
        true,
    )
    .expect("seed app-session token");

    let (rpc_addr, rpc_join) = serve_rpc().await;
    Harness {
        _tmp: tmp,
        _guards: guards,
        rpc_base: format!("http://{rpc_addr}"),
        rpc_join,
        backend_join,
    }
}

async fn rpc(rpc_base: &str, id: i64, method: &str, params: Value) -> Value {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("client");
    let url = format!("{}/rpc", rpc_base.trim_end_matches('/'));
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
        ReqwestStatusCode::OK,
        "{method} HTTP status"
    );
    response
        .json::<Value>()
        .await
        .unwrap_or_else(|err| panic!("json for {method}: {err}"))
}

fn payload<'a>(value: &'a Value, context: &str) -> &'a Value {
    if let Some(error) = value.get("error") {
        panic!("{context}: unexpected JSON-RPC error: {error}");
    }
    let result = value
        .get("result")
        .unwrap_or_else(|| panic!("{context}: missing result: {value}"));
    result.get("result").unwrap_or(result)
}

fn error_message<'a>(value: &'a Value, context: &str) -> &'a str {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{context}: missing error message: {value}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn channels_rpc_covers_credentials_managed_backend_and_error_paths() {
    let _lock = env_lock();
    let harness = setup().await;

    let list = rpc(&harness.rpc_base, 1, "openhuman.channels_list", json!({})).await;
    let channels = payload(&list, "channels_list")
        .as_array()
        .expect("channels list");
    assert!(channels
        .iter()
        .any(|channel| channel.get("id").and_then(Value::as_str) == Some("telegram")));

    let describe = rpc(
        &harness.rpc_base,
        2,
        "openhuman.channels_describe",
        json!({ "channel": " telegram " }),
    )
    .await;
    assert_eq!(
        payload(&describe, "channels_describe")
            .get("id")
            .and_then(Value::as_str),
        Some("telegram")
    );
    let unknown = rpc(
        &harness.rpc_base,
        3,
        "openhuman.channels_describe",
        json!({ "channel": "missing" }),
    )
    .await;
    assert!(error_message(&unknown, "unknown channel").contains("unknown channel"));

    let bad_mode = rpc(
        &harness.rpc_base,
        4,
        "openhuman.channels_connect",
        json!({ "channel": "telegram", "authMode": "bad_mode" }),
    )
    .await;
    assert!(error_message(&bad_mode, "bad auth mode").contains("invalid authMode"));

    let missing_creds = rpc(
        &harness.rpc_base,
        5,
        "openhuman.channels_test",
        json!({ "channel": "telegram", "authMode": "bot_token", "credentials": {} }),
    )
    .await;
    assert!(error_message(&missing_creds, "missing creds").contains("missing required fields"));

    let test_ok = rpc(
        &harness.rpc_base,
        6,
        "openhuman.channels_test",
        json!({
            "channel": "telegram",
            "authMode": "bot_token",
            "credentials": { "bot_token": "123456:test", "allowed_users": "alice, bob" }
        }),
    )
    .await;
    assert_eq!(
        payload(&test_ok, "channels_test")
            .get("success")
            .and_then(Value::as_bool),
        Some(true)
    );

    let connect_telegram = rpc(
        &harness.rpc_base,
        7,
        "openhuman.channels_connect",
        json!({
            "channel": "telegram",
            "authMode": "bot_token",
            "credentials": { "bot_token": "123456:test", "allowed_users": "alice, bob" }
        }),
    )
    .await;
    assert_eq!(
        payload(&connect_telegram, "connect telegram")
            .get("status")
            .and_then(Value::as_str),
        Some("connected")
    );

    let connect_discord = rpc(
        &harness.rpc_base,
        8,
        "openhuman.channels_connect",
        json!({
            "channel": "discord",
            "authMode": "bot_token",
            "credentials": {
                "bot_token": "discord-token",
                "guild_id": "guild-1",
                "channel_id": "channel-1",
                "allowed_users": ["u1", "u2"],
                "listen_to_bots": true,
                "mention_only": false
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&connect_discord, "connect discord")
            .get("restart_required")
            .and_then(Value::as_bool),
        Some(true)
    );

    let connect_imessage = rpc(
        &harness.rpc_base,
        9,
        "openhuman.channels_connect",
        json!({
            "channel": "imessage",
            "authMode": "managed_dm",
            "credentials": { "allowed_contacts": "mom@example.com, +15551234567" }
        }),
    )
    .await;
    assert_eq!(
        payload(&connect_imessage, "connect imessage")
            .get("status")
            .and_then(Value::as_str),
        Some("connected")
    );

    let status = rpc(
        &harness.rpc_base,
        10,
        "openhuman.channels_status",
        json!({}),
    )
    .await;
    let entries = payload(&status, "channels_status")
        .as_array()
        .expect("status entries");
    assert!(entries.iter().any(|entry| {
        entry.get("channel_id").and_then(Value::as_str) == Some("telegram")
            && entry.get("connected").and_then(Value::as_bool) == Some(true)
    }));
    assert!(entries.iter().any(|entry| {
        entry.get("channel_id").and_then(Value::as_str) == Some("imessage")
            && entry.get("connected").and_then(Value::as_bool) == Some(true)
    }));

    let filtered_status = rpc(
        &harness.rpc_base,
        11,
        "openhuman.channels_status",
        json!({ "channel": " discord " }),
    )
    .await;
    assert!(payload(&filtered_status, "filtered status")
        .as_array()
        .expect("filtered status entries")
        .iter()
        .all(|entry| entry.get("channel_id").and_then(Value::as_str) == Some("discord")));

    let telegram_start = rpc(
        &harness.rpc_base,
        12,
        "openhuman.channels_telegram_login_start",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&telegram_start, "telegram login start")
            .get("botUsername")
            .and_then(Value::as_str),
        Some("coverage_bot")
    );
    let telegram_check = rpc(
        &harness.rpc_base,
        13,
        "openhuman.channels_telegram_login_check",
        json!({ "linkToken": "telegram-link-e2e" }),
    )
    .await;
    assert_eq!(
        payload(&telegram_check, "telegram login check")
            .get("linked")
            .and_then(Value::as_bool),
        Some(true)
    );

    let discord_start = rpc(
        &harness.rpc_base,
        14,
        "openhuman.channels_discord_link_start",
        json!({}),
    )
    .await;
    assert!(payload(&discord_start, "discord link start")
        .get("instructions")
        .and_then(Value::as_str)
        .is_some_and(|instructions| instructions.contains("discord-link-e2e")));
    let discord_check = rpc(
        &harness.rpc_base,
        15,
        "openhuman.channels_discord_link_check",
        json!({ "linkToken": "discord-link-e2e" }),
    )
    .await;
    assert_eq!(
        payload(&discord_check, "discord link check")
            .get("linked")
            .and_then(Value::as_bool),
        Some(true)
    );

    let send = rpc(
        &harness.rpc_base,
        16,
        "openhuman.channels_send_message",
        json!({ "channel": "telegram", "message": { "text": "hello", "threadId": "thread-1" } }),
    )
    .await;
    assert_eq!(
        payload(&send, "send message")
            .get("messageId")
            .and_then(Value::as_str),
        Some("msg-1")
    );
    let reaction = rpc(
        &harness.rpc_base,
        17,
        "openhuman.channels_send_reaction",
        json!({ "channel": "telegram", "reaction": { "messageId": "msg-1", "emoji": "+1" } }),
    )
    .await;
    assert_eq!(
        payload(&reaction, "send reaction")
            .get("ok")
            .and_then(Value::as_bool),
        Some(true)
    );
    let create_thread = rpc(
        &harness.rpc_base,
        18,
        "openhuman.channels_create_thread",
        json!({ "channel": "telegram", "title": "Coverage Thread" }),
    )
    .await;
    assert_eq!(
        payload(&create_thread, "create thread")
            .get("threadId")
            .and_then(Value::as_str),
        Some("thread-1")
    );
    let update_thread = rpc(
        &harness.rpc_base,
        19,
        "openhuman.channels_update_thread",
        json!({ "channel": "telegram", "threadId": "thread-1", "action": "close" }),
    )
    .await;
    assert_eq!(
        payload(&update_thread, "update thread")
            .get("action")
            .and_then(Value::as_str),
        Some("close")
    );
    let list_threads = rpc(
        &harness.rpc_base,
        20,
        "openhuman.channels_list_threads",
        json!({ "channel": "telegram", "active": true }),
    )
    .await;
    assert_eq!(
        payload(&list_threads, "list threads")
            .pointer("/threads/0/threadId")
            .and_then(Value::as_str),
        Some("thread-1")
    );

    let bad_update = rpc(
        &harness.rpc_base,
        21,
        "openhuman.channels_update_thread",
        json!({ "channel": "telegram", "threadId": "thread-1", "action": "archive" }),
    )
    .await;
    assert!(error_message(&bad_update, "bad update action").contains("action must be"));

    let disconnect_telegram = rpc(
        &harness.rpc_base,
        22,
        "openhuman.channels_disconnect",
        json!({ "channel": "telegram", "authMode": "bot_token", "clearMemory": false }),
    )
    .await;
    assert_eq!(
        payload(&disconnect_telegram, "disconnect telegram")
            .get("disconnected")
            .and_then(Value::as_bool),
        Some(true)
    );

    let disconnect_imessage = rpc(
        &harness.rpc_base,
        23,
        "openhuman.channels_disconnect",
        json!({ "channel": "imessage", "authMode": "managed_dm", "clearMemory": false }),
    )
    .await;
    assert_eq!(
        payload(&disconnect_imessage, "disconnect imessage")
            .get("memory_chunks_deleted")
            .and_then(Value::as_u64),
        Some(0)
    );

    harness.rpc_join.abort();
    harness.backend_join.abort();
}

#[test]
fn tools_and_tool_registry_public_surfaces_cover_schema_and_assembly_paths() {
    let dir = tempdir().expect("tempdir");
    let mut config = Config {
        workspace_dir: dir.path().to_path_buf(),
        config_path: dir.path().join("config.toml"),
        ..Config::default()
    };
    config.node.enabled = false;
    config.browser.enabled = true;
    config.http_request.allowed_domains = vec![
        "*".to_string(),
        "docs.openhuman.ai".to_string(),
        "example.com".to_string(),
    ];
    config.gitbooks.enabled = true;
    config.computer_control.enabled = true;
    config.learning.enabled = true;
    config.learning.tool_tracking_enabled = true;
    config.mcp_client.enabled = true;

    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let memory: Arc<dyn Memory> = Arc::new(StubMemory);
    let tools = all_tools(
        Arc::new(config.clone()),
        &security,
        AuditLogger::disabled(),
        memory,
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &HashMap::new(),
        &config,
    );
    let names = tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();
    for expected in [
        "shell",
        "file_read",
        "grep",
        "browser_open",
        "browser",
        "http_request",
        "web_fetch",
        "curl",
        "gitbooks_search",
        "gitbooks_get_page",
        "mouse",
        "keyboard",
        "tool_stats",
        "screenshot",
        "image_info",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}; got {names:?}"
        );
    }
    assert!(!names.contains(&"node_exec"));
    assert!(!names.contains(&"npm_exec"));

    let baseline = default_tools(security);
    assert_eq!(baseline.len(), 3);
    assert_eq!(baseline[0].scope(), ToolScope::All);
    assert_eq!(baseline[0].permission_level(), PermissionLevel::Execute);

    let wrappers = tools_wrappers_list_json();
    assert!(wrappers
        .pointer("/result/wrappers")
        .and_then(Value::as_array)
        .expect("wrapper list")
        .iter()
        .any(|wrapper| wrapper.get("name").and_then(Value::as_str) == Some("screenshot")));

    let tool_schemas = all_tools_controller_schemas();
    let tool_controllers = all_tools_registered_controllers();
    assert_eq!(tool_schemas.len(), tool_controllers.len());
    assert!(tool_schemas
        .iter()
        .any(|schema| schema.function == "web_search"));

    let registry_schemas = all_tool_registry_controller_schemas();
    let registry_controllers = all_tool_registry_registered_controllers();
    assert_eq!(registry_schemas.len(), 3);
    assert_eq!(registry_schemas.len(), registry_controllers.len());
    assert!(registry_entries()
        .iter()
        .any(|entry| entry.tool_id == "tools.web_search"));

    let dirty_schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "kind": {
                "anyOf": [
                    { "type": "string", "const": "alpha" },
                    { "type": "string", "const": "beta" }
                ]
            },
            "age": { "$ref": "#/$defs/Age", "description": "age field" },
            "nullable": { "type": ["string", "null"] },
            "unresolved": { "$ref": "#/$defs/Missing", "title": "missing ref" },
            "cycle": { "$ref": "#/$defs/Cycle", "description": "cycle ref" }
        },
        "$defs": {
            "Age": { "type": "integer", "minimum": 0 },
            "Cycle": { "$ref": "#/$defs/Cycle" }
        }
    });
    let gemini = SchemaCleanr::clean_for_gemini(dirty_schema.clone());
    assert_eq!(
        gemini.pointer("/properties/kind/type"),
        Some(&json!("string"))
    );
    assert_eq!(
        gemini.pointer("/properties/kind/enum"),
        Some(&json!(["alpha", "beta"]))
    );
    assert_eq!(
        gemini.pointer("/properties/age/type"),
        Some(&json!("integer"))
    );
    assert_eq!(
        gemini.pointer("/properties/age/description"),
        Some(&json!("age field"))
    );
    assert_eq!(
        gemini.pointer("/properties/nullable/type"),
        Some(&json!("string"))
    );
    assert_eq!(
        gemini.pointer("/properties/unresolved/title"),
        Some(&json!("missing ref"))
    );
    assert!(SchemaCleanr::validate(&gemini).is_ok());
    assert!(SchemaCleanr::validate(&json!("not-object")).is_err());
    assert!(SchemaCleanr::validate(&json!({ "properties": {} })).is_err());

    let anthropic = SchemaCleanr::clean(dirty_schema.clone(), CleaningStrategy::Anthropic);
    assert!(anthropic.get("$defs").is_none());
    let openai = SchemaCleanr::clean_for_openai(dirty_schema);
    assert!(openai.get("$defs").is_some());

    let policy = DefaultToolPolicy;
    assert_eq!(
        policy.evaluate("anything", &json!({ "arg": true })),
        PolicyDecision::Allow
    );
}
