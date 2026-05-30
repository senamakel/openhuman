//! Raw-line oriented integration coverage for tools, approval, channels, and
//! tool_registry surfaces that are not covered by the narrower controller tests.

use std::collections::{BTreeSet, HashMap};
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
use openhuman_core::openhuman::channels::email_channel::EmailConfig;
use openhuman_core::openhuman::channels::traits::ChannelMessage;
use openhuman_core::openhuman::channels::yuanbao::types::{
    Account as YuanbaoAccount, ConnFrame as YuanbaoConnFrame,
    ConnectionState as YuanbaoConnectionState, GroupInfo as YuanbaoGroupInfo,
    GroupMember as YuanbaoGroupMember, GroupMemberListPage as YuanbaoGroupMemberListPage,
    ImMsgSeq as YuanbaoImMsgSeq, ImageInfo as YuanbaoImageInfo,
    InboundMessage as YuanbaoInboundMessage, MessageKind as YuanbaoMessageKind,
    MsgBodyElement as YuanbaoMsgBodyElement, MsgContent as YuanbaoMsgContent,
    Source as YuanbaoSource,
};
use openhuman_core::openhuman::channels::{
    Channel, CliChannel, DingTalkChannel, EmailChannel, IMessageChannel, LinqChannel,
    MattermostChannel, QQChannel, SendMessage, SignalChannel, SlackChannel, WhatsAppChannel,
};
use openhuman_core::openhuman::composio::all_composio_agent_tools;
use openhuman_core::openhuman::config::schema::{
    CapabilityProviderConfig, CapabilityProviderTrustState,
};
use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::credentials::{
    AuthService, APP_SESSION_PROVIDER, DEFAULT_AUTH_PROFILE_NAME,
};
use openhuman_core::openhuman::memory::{
    Memory, MemoryCategory, MemoryEntry, NamespaceSummary, RecallOpts,
};
use openhuman_core::openhuman::security::{AuditLogger, SecurityPolicy};
use openhuman_core::openhuman::tool_registry::ops::diagnostics_for_config;
use openhuman_core::openhuman::tool_registry::{
    all_tool_registry_controller_schemas, all_tool_registry_registered_controllers,
    capability_provider_by_id, capability_provider_diagnostics, capability_provider_registry,
    denials, get_tool, is_capability_provider_trusted_enabled, list_capability_providers,
    list_tools, normalize_capability_provider_id, registry_entries,
    CapabilityProviderRegistryError,
};
use openhuman_core::openhuman::tools::generated::{
    admit_generated_tool_definitions, generated_tools_from_definitions, GeneratedToolAdapter,
    GeneratedToolAdmissionConfig, GeneratedToolDefinition, GeneratedToolRisk,
};
use openhuman_core::openhuman::tools::local_cli::tools_wrappers_list_json;
use openhuman_core::openhuman::tools::{
    all_tools, all_tools_controller_schemas, all_tools_registered_controllers, default_tools,
    CleaningStrategy, DefaultToolPolicy, PermissionLevel, PolicyDecision, SchemaCleanr, Tool,
    ToolCallOptions, ToolCategory, ToolPolicy, ToolResult, ToolScope,
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

struct EchoGeneratedAdapter;

#[async_trait]
impl GeneratedToolAdapter for EchoGeneratedAdapter {
    fn id(&self) -> &str {
        "echo-generated"
    }

    async fn execute(
        &self,
        definition: &GeneratedToolDefinition,
        args: Value,
    ) -> Result<ToolResult> {
        Ok(ToolResult::success(
            json!({
                "tool": definition.name,
                "adapter": definition.adapter_id,
                "args": args
            })
            .to_string(),
        ))
    }
}

struct DefaultPathTool;

#[async_trait]
impl openhuman_core::openhuman::tools::Tool for DefaultPathTool {
    fn name(&self) -> &str {
        "default_path_tool"
    }

    fn description(&self) -> &str {
        "Covers default Tool trait metadata paths."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "value": { "type": "string" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<ToolResult> {
        Ok(ToolResult::success(args.to_string()))
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
        (Method::GET, "/agent-integrations/composio/toolkits") => json!({
            "success": true,
            "data": { "toolkits": ["gmail", "github", "slack"] }
        }),
        (Method::GET, "/agent-integrations/composio/connections") => json!({
            "success": true,
            "data": {
                "connections": [
                    {
                        "id": "conn-gmail-1",
                        "toolkit": " Gmail ",
                        "status": "ACTIVE",
                        "createdAt": "2026-05-29T12:00:00Z"
                    },
                    {
                        "id": "conn-slack-pending",
                        "toolkit": "slack",
                        "status": "pending",
                        "createdAt": "2026-05-29T12:05:00Z"
                    }
                ]
            }
        }),
        (Method::POST, "/agent-integrations/composio/authorize") => json!({
            "success": true,
            "data": {
                "connectUrl": format!(
                    "https://connect.example.test/{}",
                    json_body.get("toolkit").and_then(Value::as_str).unwrap_or("unknown")
                ),
                "connectionId": "conn-new-1"
            }
        }),
        (Method::GET, "/agent-integrations/composio/tools") => json!({
            "success": true,
            "data": {
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "GMAIL_FETCH_EMAILS",
                            "description": "Fetch matching Gmail messages for the user.",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string" },
                                    "maxResults": { "type": "integer" }
                                },
                                "required": ["query"]
                            }
                        }
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "GMAIL_UNCURATED_INTERNAL",
                            "description": "Backend-only action that should be filtered out.",
                            "parameters": { "type": "object", "properties": {} }
                        }
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "SLACK_POST_MESSAGE",
                            "description": "Slack write action for an unconnected toolkit.",
                            "parameters": {
                                "type": "object",
                                "properties": { "text": { "type": "string" } },
                                "required": ["text"]
                            }
                        }
                    }
                ]
            }
        }),
        (Method::POST, "/agent-integrations/composio/execute") => json!({
            "success": true,
            "data": {
                "data": {
                    "tool": json_body.get("tool").cloned().unwrap_or(Value::Null),
                    "arguments": json_body.get("arguments").cloned().unwrap_or(Value::Null)
                },
                "successful": true,
                "error": null,
                "costUsd": 0.015,
                "markdownFormatted": "Fetched 1 matching Gmail message."
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
async fn composio_agent_tools_cover_backend_discovery_markdown_and_execution_paths() {
    let _lock = env_lock();
    let harness = setup().await;
    let config = Config::load_or_init()
        .await
        .expect("load config for composio tools");
    let tools = all_composio_agent_tools(&config);
    let names = tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "composio_list_toolkits",
            "composio_list_connections",
            "composio_authorize",
            "composio_list_tools",
            "composio_execute",
        ]
    );
    assert!(tools
        .iter()
        .all(|tool| tool.category() == ToolCategory::Skill));

    let list_toolkits = tools
        .iter()
        .find(|tool| tool.name() == "composio_list_toolkits")
        .expect("list toolkits tool");
    let toolkits = list_toolkits
        .execute(json!({}))
        .await
        .expect("list toolkits executes");
    assert!(!toolkits.is_error, "{}", toolkits.output());
    assert!(toolkits.output().contains("gmail"));

    let list_connections = tools
        .iter()
        .find(|tool| tool.name() == "composio_list_connections")
        .expect("list connections tool");
    let connections = list_connections
        .execute(json!({}))
        .await
        .expect("list connections executes");
    assert!(!connections.is_error, "{}", connections.output());
    assert!(connections.output().contains("conn-gmail-1"));
    assert!(
        !connections.output().contains("conn-slack-pending"),
        "pending connections should be filtered before reaching the agent"
    );

    let list_tools = tools
        .iter()
        .find(|tool| tool.name() == "composio_list_tools")
        .expect("list tools tool");
    assert!(list_tools.supports_markdown());
    let discovered = list_tools
        .execute_with_options(
            json!({
                "toolkits": [" gmail "],
                "tags": ["readOnlyHint"],
                "include_unconnected": false
            }),
            ToolCallOptions {
                prefer_markdown: true,
            },
        )
        .await
        .expect("list tools executes");
    assert!(!discovered.is_error, "{}", discovered.output());
    assert!(discovered.output().contains("GMAIL_FETCH_EMAILS"));
    assert!(!discovered.output().contains("GMAIL_UNCURATED_INTERNAL"));
    assert!(!discovered.output().contains("SLACK_POST_MESSAGE"));
    let markdown = discovered
        .markdown_formatted
        .as_deref()
        .expect("markdown rendering");
    assert!(markdown.contains("# Composio tools"));
    assert!(markdown.contains("**req:** query"));
    assert!(markdown.contains("**opt:** maxResults"));

    let authorize = tools
        .iter()
        .find(|tool| tool.name() == "composio_authorize")
        .expect("authorize tool");
    let missing_toolkit = authorize
        .execute(json!({}))
        .await
        .expect("authorize validates params");
    assert!(missing_toolkit.is_error);
    assert!(missing_toolkit.output().contains("'toolkit' is required"));
    let handoff = authorize
        .execute(json!({ "toolkit": "gmail" }))
        .await
        .expect("authorize executes");
    assert!(!handoff.is_error, "{}", handoff.output());
    assert!(handoff
        .output()
        .contains("https://connect.example.test/gmail"));
    assert!(handoff.output().contains("conn-new-1"));

    let execute = tools
        .iter()
        .find(|tool| tool.name() == "composio_execute")
        .expect("execute tool");
    let missing_action = execute
        .execute(json!({ "arguments": {} }))
        .await
        .expect("execute validates tool");
    assert!(missing_action.is_error);
    assert!(missing_action.output().contains("'tool' is required"));
    let executed = execute
        .execute(json!({
            "tool": "GMAIL_FETCH_EMAILS",
            "connection_id": "conn-gmail-1",
            "arguments": { "query": "from:alice@example.test", "maxResults": 1 }
        }))
        .await
        .expect("execute dispatches");
    assert!(!executed.is_error, "{}", executed.output());
    assert_eq!(executed.output(), "Fetched 1 matching Gmail message.");

    harness.rpc_join.abort();
    harness.backend_join.abort();
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

    let unsupported_mode = rpc(
        &harness.rpc_base,
        24,
        "openhuman.channels_connect",
        json!({ "channel": "web", "authMode": "bot_token", "credentials": {} }),
    )
    .await;
    assert!(error_message(&unsupported_mode, "unsupported auth mode").contains("does not support"));

    let non_object_creds = rpc(
        &harness.rpc_base,
        25,
        "openhuman.channels_test",
        json!({ "channel": "telegram", "authMode": "bot_token", "credentials": "bad" }),
    )
    .await;
    assert!(error_message(&non_object_creds, "non-object credentials")
        .contains("credentials must be a JSON object"));

    let telegram_managed = rpc(
        &harness.rpc_base,
        26,
        "openhuman.channels_connect",
        json!({ "channel": "telegram", "authMode": "managed_dm", "credentials": {} }),
    )
    .await;
    assert_eq!(
        payload(&telegram_managed, "connect telegram managed dm")
            .get("status")
            .and_then(Value::as_str),
        Some("pending_auth")
    );
    assert_eq!(
        payload(&telegram_managed, "connect telegram managed dm")
            .get("auth_action")
            .and_then(Value::as_str),
        Some("telegram_managed_dm")
    );

    let discord_oauth = rpc(
        &harness.rpc_base,
        27,
        "openhuman.channels_connect",
        json!({ "channel": "discord", "authMode": "oauth", "credentials": {} }),
    )
    .await;
    assert_eq!(
        payload(&discord_oauth, "connect discord oauth")
            .get("auth_action")
            .and_then(Value::as_str),
        Some("discord_oauth")
    );

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

    let lark_test = rpc(
        &harness.rpc_base,
        28,
        "openhuman.channels_test",
        json!({
            "channel": "lark",
            "authMode": "api_key",
            "credentials": {
                "app_id": "cli_lark",
                "app_secret": "lark-secret",
                "use_feishu": "yes",
                "allowed_users": [" user-a ", "@user-b,user-a"]
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&lark_test, "test lark")
            .get("success")
            .and_then(Value::as_bool),
        Some(true)
    );

    let connect_lark = rpc(
        &harness.rpc_base,
        29,
        "openhuman.channels_connect",
        json!({
            "channel": "lark",
            "authMode": "api_key",
            "credentials": {
                "app_id": "cli_lark",
                "app_secret": "lark-secret",
                "receive_mode": "websocket",
                "port": "8080"
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&connect_lark, "connect lark")
            .get("status")
            .and_then(Value::as_str),
        Some("connected")
    );

    let connect_dingtalk = rpc(
        &harness.rpc_base,
        30,
        "openhuman.channels_connect",
        json!({
            "channel": "dingtalk",
            "authMode": "api_key",
            "credentials": {
                "client_id": "ding-client",
                "client_secret": "ding-secret",
                "allowed_users": "u1\n@u2"
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&connect_dingtalk, "connect dingtalk")
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
    assert!(entries.iter().any(|entry| {
        entry.get("channel_id").and_then(Value::as_str) == Some("lark")
            && entry.get("connected").and_then(Value::as_bool) == Some(true)
    }));
    assert!(entries.iter().any(|entry| {
        entry.get("channel_id").and_then(Value::as_str) == Some("dingtalk")
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

    let disconnect_lark = rpc(
        &harness.rpc_base,
        31,
        "openhuman.channels_disconnect",
        json!({ "channel": "lark", "authMode": "api_key", "clearMemory": false }),
    )
    .await;
    assert_eq!(
        payload(&disconnect_lark, "disconnect lark")
            .get("disconnected")
            .and_then(Value::as_bool),
        Some(true)
    );

    let disconnect_dingtalk = rpc(
        &harness.rpc_base,
        32,
        "openhuman.channels_disconnect",
        json!({ "channel": "dingtalk", "authMode": "api_key", "clearMemory": false }),
    )
    .await;
    assert_eq!(
        payload(&disconnect_dingtalk, "disconnect dingtalk")
            .get("disconnected")
            .and_then(Value::as_bool),
        Some(true)
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
    let listed = list_tools()
        .into_cli_compatible_json()
        .expect("list_tools json");
    let listed_tools = listed
        .get("tools")
        .and_then(Value::as_array)
        .expect("listed tools");
    let first_tool_id = listed_tools
        .first()
        .and_then(|tool| tool.get("tool_id"))
        .and_then(Value::as_str)
        .expect("first registry tool id");
    let found = get_tool(first_tool_id)
        .expect("get first registry tool")
        .into_cli_compatible_json()
        .expect("get_tool json");
    assert_eq!(
        found.get("tool_id").and_then(Value::as_str),
        Some(first_tool_id)
    );
    assert!(get_tool("  ")
        .expect_err("blank registry id should fail")
        .contains("non-empty"));
    assert!(get_tool("tools.missing")
        .expect_err("missing registry id should fail")
        .contains("tools.missing"));

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

    let default_tool = DefaultPathTool;
    let spec = default_tool.spec();
    assert_eq!(spec.name, "default_path_tool");
    assert_eq!(
        spec.description,
        "Covers default Tool trait metadata paths."
    );
    assert_eq!(default_tool.permission_level(), PermissionLevel::ReadOnly);
    assert_eq!(
        default_tool.permission_level_with_args(&json!({ "value": "x" })),
        PermissionLevel::ReadOnly
    );
    assert_eq!(default_tool.scope(), ToolScope::All);
    assert_eq!(default_tool.category(), ToolCategory::System);
    assert!(!default_tool.supports_markdown());
    assert!(!default_tool.is_concurrency_safe(&json!({})));
    assert!(!default_tool.external_effect());
    assert!(!default_tool.external_effect_with_args(&json!({})));
    assert!(default_tool.generated_runtime_context(&json!({})).is_none());
    assert!(default_tool.max_result_size_chars().is_none());
}

#[tokio::test]
async fn channels_public_helpers_cover_cli_trait_defaults_and_webhook_parsers() {
    let cli = CliChannel::new();
    assert_eq!(cli.name(), "cli");
    assert!(cli
        .send(&SendMessage::new("hello from coverage", "stdout"))
        .await
        .is_ok());
    assert!(cli.health_check().await);
    assert!(cli.start_typing("user").await.is_ok());
    assert!(cli.stop_typing("user").await.is_ok());
    assert!(!cli.supports_reactions());
    assert!(!cli.supports_draft_updates());
    assert!(cli
        .send_draft(&SendMessage::new("draft", "user"))
        .await
        .expect("default send_draft")
        .is_none());
    assert!(cli.update_draft("user", "msg-1", "draft").await.is_ok());
    assert!(cli
        .finalize_draft("user", "msg-1", "final", Some("thread-1"))
        .await
        .is_ok());

    let threaded = SendMessage::with_subject("body", "recipient", "subject")
        .in_thread(Some("thread-1".to_string()));
    assert_eq!(threaded.subject.as_deref(), Some("subject"));
    assert_eq!(threaded.thread_ts.as_deref(), Some("thread-1"));

    let whatsapp = WhatsAppChannel::new(
        "token".into(),
        "phone-id".into(),
        "verify-me".into(),
        vec!["+15551234567".into()],
    );
    assert_eq!(whatsapp.name(), "whatsapp");
    assert_eq!(whatsapp.verify_token(), "verify-me");
    let whatsapp_messages = whatsapp.parse_webhook_payload(&json!({
        "entry": [{
            "changes": [{
                "value": {
                    "messages": [{
                        "id": "wamid.1",
                        "from": "15551234567",
                        "timestamp": "1780000000",
                        "text": { "body": "hi from whatsapp" }
                    }, {
                        "id": "wamid.2",
                        "from": "15550000000",
                        "timestamp": "1780000001",
                        "text": { "body": "blocked" }
                    }, {
                        "id": "wamid.3",
                        "from": "15551234567",
                        "timestamp": "bad",
                        "image": { "id": "media" }
                    }]
                }
            }]
        }]
    }));
    assert_eq!(whatsapp_messages.len(), 1);
    assert_eq!(whatsapp_messages[0].sender, "+15551234567");
    assert_eq!(whatsapp_messages[0].content, "hi from whatsapp");
    assert!(whatsapp.parse_webhook_payload(&json!({})).is_empty());

    let linq = LinqChannel::new("linq-token".into(), "+15557654321".into(), vec!["*".into()]);
    assert_eq!(linq.name(), "linq");
    assert_eq!(linq.phone_number(), "+15557654321");
    let linq_messages = linq.parse_webhook_payload(&json!({
        "event_type": "message.received",
        "data": {
            "chat_id": "chat-1",
            "from": "15551234567",
            "recipient_phone": "+15557654321",
            "service": "iMessage",
            "is_from_me": false,
            "message": {
                "id": "linq-msg-1",
                "parts": [
                    { "type": "text", "value": "hello" },
                    { "type": "media", "url": "https://cdn.example.test/image.png", "mime_type": "image/png" },
                    { "type": "media", "url": "https://cdn.example.test/file.pdf", "mime_type": "application/pdf" }
                ]
            }
        }
    }));
    assert_eq!(linq_messages.len(), 1);
    assert_eq!(linq_messages[0].sender, "+15551234567");
    assert!(linq_messages[0].content.contains("hello"));
    assert!(linq_messages[0]
        .content
        .contains("[IMAGE:https://cdn.example.test/image.png]"));
    assert!(linq
        .parse_webhook_payload(&json!({ "event_type": "message.sent" }))
        .is_empty());
    assert!(linq
        .parse_webhook_payload(&json!({
            "event_type": "message.received",
            "data": { "is_from_me": true }
        }))
        .is_empty());
}

#[tokio::test]
async fn channel_provider_public_paths_cover_pre_network_errors_and_utilities() {
    let dingtalk = DingTalkChannel::new("client".into(), "secret".into(), vec!["*".into()]);
    assert_eq!(dingtalk.name(), "dingtalk");
    let missing_webhook = dingtalk
        .send(&SendMessage::new("reply", "chat-without-session"))
        .await
        .expect_err("dingtalk send should fail before network without a session webhook");
    assert!(missing_webhook.to_string().contains("No session webhook"));

    let slack = SlackChannel::new("xoxb-test".into(), None, vec!["U1".into()]);
    assert_eq!(slack.name(), "slack");
    let (tx, _rx) = tokio::sync::mpsc::channel::<ChannelMessage>(1);
    let slack_listen = slack
        .listen(tx)
        .await
        .expect_err("slack listen should require channel_id before polling");
    assert!(slack_listen.to_string().contains("channel_id required"));

    let mattermost = MattermostChannel::new(
        "https://mattermost.example.test///".into(),
        "token".into(),
        None,
        vec!["*".into()],
        true,
        false,
    );
    assert_eq!(mattermost.name(), "mattermost");
    let (tx, _rx) = tokio::sync::mpsc::channel::<ChannelMessage>(1);
    let mattermost_listen = mattermost
        .listen(tx)
        .await
        .expect_err("mattermost listen should require channel_id before polling");
    assert!(mattermost_listen
        .to_string()
        .contains("channel_id required"));
    assert!(mattermost.stop_typing("channel:root").await.is_ok());

    let imessage = IMessageChannel::new(vec!["friend@example.test".into()]);
    assert_eq!(imessage.name(), "imessage");
    let invalid_target = imessage
        .send(&SendMessage::new("hello", "not a valid recipient"))
        .await
        .expect_err("invalid iMessage target should be rejected before osascript");
    assert!(invalid_target
        .to_string()
        .contains("Invalid iMessage target"));

    let mut email_config = EmailConfig {
        allowed_senders: vec![
            "ALICE@example.test".into(),
            "@trusted.test".into(),
            "domain.test".into(),
        ],
        ..EmailConfig::default()
    };
    assert_eq!(email_config.imap_port, 993);
    assert_eq!(email_config.smtp_port, 465);
    assert_eq!(email_config.imap_folder, "INBOX");
    assert!(email_config.smtp_tls);
    let email = EmailChannel::new(email_config.clone());
    assert_eq!(email.name(), "email");
    assert!(email.is_sender_allowed("alice@example.test"));
    assert!(email.is_sender_allowed("alerts@trusted.test"));
    assert!(email.is_sender_allowed("bot@domain.test"));
    assert!(!email.is_sender_allowed("mallory@example.test"));
    assert_eq!(
        EmailChannel::strip_html("<div>Hello<br><strong>friend</strong></div>"),
        "Hellofriend"
    );
    email_config.allowed_senders = vec!["*".into()];
    assert!(EmailChannel::new(email_config).is_sender_allowed("anyone@elsewhere.test"));

    let qq = QQChannel::new("app".into(), "secret".into(), vec!["*".into()]);
    assert_eq!(qq.name(), "qq");
    let signal = SignalChannel::new(
        "http://127.0.0.1:1///".into(),
        "+15551234567".into(),
        Some("dm".into()),
        vec!["*".into()],
        true,
        true,
    );
    assert_eq!(signal.name(), "signal");
}

#[test]
fn yuanbao_shared_types_cover_message_extractors_and_state_variants() {
    let frame = YuanbaoConnFrame {
        cmd_type: 2,
        cmd: "push".into(),
        module: "yuanbao_openclaw_proxy".into(),
        seq_no: 42,
        msg_id: "frame-1".into(),
        need_ack: true,
        status: 0,
        data: vec![1, 2, 3],
    };
    assert_eq!(frame.cmd_type, 2);
    assert_eq!(frame.cmd, "push");
    assert_eq!(frame.module, "yuanbao_openclaw_proxy");
    assert_eq!(frame.seq_no, 42);
    assert_eq!(frame.msg_id, "frame-1");
    assert!(frame.need_ack);
    assert_eq!(frame.status, 0);
    assert_eq!(frame.data, vec![1, 2, 3]);

    let text = YuanbaoMsgBodyElement {
        msg_type: "TIMTextElem".into(),
        msg_content: YuanbaoMsgContent {
            text: Some("first".into()),
            ..Default::default()
        },
    };
    let second_text = YuanbaoMsgBodyElement {
        msg_type: "TIMTextElem".into(),
        msg_content: YuanbaoMsgContent {
            text: Some("second".into()),
            ..Default::default()
        },
    };
    let image = YuanbaoMsgBodyElement {
        msg_type: "TIMImageElem".into(),
        msg_content: YuanbaoMsgContent {
            uuid: Some("uuid".into()),
            image_format: Some(3),
            data: Some("inline-data".into()),
            desc: Some("image desc".into()),
            ext: Some("{}".into()),
            sound: Some("sound-id".into()),
            image_info_array: vec![
                YuanbaoImageInfo {
                    image_type: 1,
                    size: 100,
                    width: 640,
                    height: 480,
                    url: "https://cdn.example.test/original.png".into(),
                },
                YuanbaoImageInfo {
                    image_type: 3,
                    size: 10,
                    width: 64,
                    height: 48,
                    url: String::new(),
                },
            ],
            index: Some(1),
            url: Some("https://cdn.example.test/original.png".into()),
            file_size: Some(100),
            file_name: Some("photo.png".into()),
            ..Default::default()
        },
    };

    let message = YuanbaoInboundMessage {
        callback_command: "C2C.Callback".into(),
        from_account: "sender-1".into(),
        to_account: "bot-1".into(),
        sender_nickname: "Alice".into(),
        group_id: "group-id".into(),
        group_code: "group-code".into(),
        group_name: "Coverage Group".into(),
        msg_seq: 7,
        msg_random: 9,
        msg_time: 1_780_000_000,
        msg_key: "key".into(),
        msg_id: "msg-1".into(),
        msg_body: vec![text, image, second_text],
        cloud_custom_data: "{}".into(),
        event_time: 1_780_000_001,
        bot_owner_id: "owner".into(),
        recall_msg_seq_list: vec![YuanbaoImMsgSeq {
            msg_seq: 6,
            msg_id: "old-msg".into(),
        }],
        claw_msg_type: 1,
        private_from_group_code: "private-group".into(),
        trace_id: "trace".into(),
    };
    assert!(message.is_group());
    assert!(message.is_recall());
    assert_eq!(message.chat_id(), "group-code");
    assert_eq!(message.extract_text(), "first\nsecond");
    assert_eq!(
        message.extract_image_urls(),
        vec!["https://cdn.example.test/original.png".to_string()]
    );
    assert_eq!(message.callback_command, "C2C.Callback");
    assert_eq!(message.to_account, "bot-1");
    assert_eq!(message.sender_nickname, "Alice");
    assert_eq!(message.group_id, "group-id");
    assert_eq!(message.group_name, "Coverage Group");
    assert_eq!(message.msg_seq, 7);
    assert_eq!(message.msg_random, 9);
    assert_eq!(message.msg_time, 1_780_000_000);
    assert_eq!(message.msg_key, "key");
    assert_eq!(message.cloud_custom_data, "{}");
    assert_eq!(message.event_time, 1_780_000_001);
    assert_eq!(message.bot_owner_id, "owner");
    assert_eq!(message.claw_msg_type, 1);
    assert_eq!(message.private_from_group_code, "private-group");
    assert_eq!(message.trace_id, "trace");

    let dm = YuanbaoInboundMessage {
        from_account: "dm-user".into(),
        ..Default::default()
    };
    assert!(!dm.is_group());
    assert!(!dm.is_recall());
    assert_eq!(dm.chat_id(), "dm-user");
    assert!(dm.extract_text().is_empty());
    assert!(dm.extract_image_urls().is_empty());

    let group_source = YuanbaoSource {
        from_account: "sender".into(),
        sender_nickname: "Alice".into(),
        group_code: "group-code".into(),
        is_group: true,
    };
    assert_eq!(group_source.reply_target(), "g:group-code");
    assert_eq!(group_source.sender_nickname, "Alice");
    let dm_source = YuanbaoSource {
        from_account: "sender".into(),
        is_group: false,
        ..Default::default()
    };
    assert_eq!(dm_source.reply_target(), "sender");

    for kind in [
        YuanbaoMessageKind::Text,
        YuanbaoMessageKind::Image,
        YuanbaoMessageKind::File,
        YuanbaoMessageKind::Voice,
        YuanbaoMessageKind::Mixed,
        YuanbaoMessageKind::Recall,
    ] {
        assert_eq!(kind, kind);
    }
    assert_eq!(YuanbaoMessageKind::default(), YuanbaoMessageKind::Text);

    let group_info = YuanbaoGroupInfo {
        code: 0,
        message: "ok".into(),
        group_name: "Coverage Group".into(),
        owner_id: "owner".into(),
        owner_nickname: "Owner".into(),
        member_count: 2,
    };
    assert_eq!(group_info.owner_nickname, "Owner");
    assert_eq!(group_info.member_count, 2);
    let member = YuanbaoGroupMember {
        user_id: "member-1".into(),
        nickname: "Member".into(),
        role: 1,
        join_time: 123,
        name_card: "Card".into(),
    };
    let page = YuanbaoGroupMemberListPage {
        code: 0,
        message: "ok".into(),
        members: vec![member],
        next_offset: 20,
        is_complete: false,
    };
    assert_eq!(page.members[0].role, 1);
    assert_eq!(page.next_offset, 20);
    assert!(!page.is_complete);

    let account = YuanbaoAccount {
        uid: "bot".into(),
        nickname: "Coverage Bot".into(),
        connect_id: "connect".into(),
    };
    let account_json = serde_json::to_string(&account).expect("serialize yuanbao account");
    assert!(account_json.contains("Coverage Bot"));
    let decoded: YuanbaoAccount =
        serde_json::from_str(&account_json).expect("deserialize yuanbao account");
    assert_eq!(decoded.connect_id, "connect");

    assert_ne!(
        YuanbaoConnectionState::Disconnected,
        YuanbaoConnectionState::Connecting
    );
    assert_eq!(
        YuanbaoConnectionState::Authenticating,
        YuanbaoConnectionState::Authenticating
    );
    assert_eq!(
        YuanbaoConnectionState::Connected,
        YuanbaoConnectionState::Connected
    );
    assert_eq!(
        YuanbaoConnectionState::Reconnecting,
        YuanbaoConnectionState::Reconnecting
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_registry_rpc_controllers_cover_list_get_diagnostics_and_errors() {
    let _lock = env_lock();
    let harness = setup().await;

    let list = rpc(
        &harness.rpc_base,
        101,
        "openhuman.tool_registry_list",
        json!({}),
    )
    .await;
    let tools = payload(&list, "tool_registry_list")
        .get("tools")
        .and_then(Value::as_array)
        .expect("registry tools");
    assert!(tools
        .iter()
        .any(|tool| tool.get("tool_id").and_then(Value::as_str) == Some("tools.web_search")));

    let found = rpc(
        &harness.rpc_base,
        102,
        "openhuman.tool_registry_get",
        json!({ "tool_id": " tools.web_search " }),
    )
    .await;
    assert_eq!(
        payload(&found, "tool_registry_get")
            .get("tool_id")
            .and_then(Value::as_str),
        Some("tools.web_search")
    );

    let missing_id = rpc(
        &harness.rpc_base,
        103,
        "openhuman.tool_registry_get",
        json!({ "tool_id": "   " }),
    )
    .await;
    assert!(error_message(&missing_id, "tool_registry_get blank id")
        .contains("tool_id must be a non-empty string"));
    let unknown_tool = rpc(
        &harness.rpc_base,
        104,
        "openhuman.tool_registry_get",
        json!({ "tool_id": "tools.not_real" }),
    )
    .await;
    assert!(
        error_message(&unknown_tool, "tool_registry_get missing tool").contains("tool not found")
    );

    let diagnostics = rpc(
        &harness.rpc_base,
        105,
        "openhuman.tool_registry_diagnostics",
        json!({}),
    )
    .await;
    let diagnostics = payload(&diagnostics, "tool_registry_diagnostics");
    assert!(diagnostics
        .get("total_tools")
        .and_then(Value::as_u64)
        .is_some_and(|count| count > 0));
    assert!(diagnostics
        .pointer("/mcp_allowlists/server_count")
        .and_then(Value::as_u64)
        .is_some());
    assert!(diagnostics
        .pointer("/mcp_write_audit/enabled")
        .and_then(Value::as_bool)
        .is_some());

    harness.rpc_join.abort();
    harness.backend_join.abort();
}

#[test]
fn tool_registry_provider_and_denial_paths_cover_diagnostics() {
    assert_eq!(
        normalize_capability_provider_id(" Trusted Runtime.Provider "),
        Ok("trusted-runtime.provider".to_string())
    );
    assert!(matches!(
        normalize_capability_provider_id("!!!"),
        Err(CapabilityProviderRegistryError::InvalidId { .. })
    ));
    let empty_provider_id_error =
        normalize_capability_provider_id("   ").expect_err("empty provider id should fail");
    assert_eq!(
        empty_provider_id_error.to_string(),
        "invalid provider id: \"\""
    );
    assert!(matches!(
        normalize_capability_provider_id(&"x".repeat(128)),
        Err(CapabilityProviderRegistryError::InvalidId { .. })
    ));

    let mut config = Config::default();
    config.capability_providers = vec![
        CapabilityProviderConfig {
            id: "Trusted Runtime.Provider".into(),
            display_name: "  Trusted Runtime  ".into(),
            source_uri: Some(" https://example.test/catalog.json ".into()),
            source_digest: Some(" sha256:abc123 ".into()),
            trust_state: CapabilityProviderTrustState::Trusted,
            enabled: true,
        },
        CapabilityProviderConfig {
            id: "Disabled Provider".into(),
            display_name: String::new(),
            source_uri: Some("   ".into()),
            source_digest: None,
            trust_state: CapabilityProviderTrustState::Untrusted,
            enabled: false,
        },
    ];

    let registry = capability_provider_registry(&config).expect("provider registry");
    assert_eq!(registry.list().len(), 2);
    assert!(registry.is_trusted_enabled("trusted runtime.provider"));
    assert!(!registry.is_trusted_enabled("disabled provider"));
    assert_eq!(
        registry
            .get("disabled provider")
            .expect("disabled provider")
            .display_name,
        "disabled-provider"
    );
    assert_eq!(
        list_capability_providers(&config)
            .expect("list providers")
            .len(),
        2
    );
    assert_eq!(
        capability_provider_by_id(&config, "TRUSTED RUNTIME.PROVIDER")
            .expect("provider lookup")
            .expect("provider")
            .source_uri
            .as_deref(),
        Some("https://example.test/catalog.json")
    );
    assert!(is_capability_provider_trusted_enabled(
        &config,
        "trusted runtime.provider"
    ));
    let provider_diagnostics = capability_provider_diagnostics(&config);
    assert_eq!(provider_diagnostics.total_providers, 2);
    assert_eq!(provider_diagnostics.enabled_providers, 1);
    assert_eq!(provider_diagnostics.trusted_providers, 1);
    assert_eq!(provider_diagnostics.trusted_enabled_providers, 1);

    let mut duplicate_config = config.clone();
    duplicate_config
        .capability_providers
        .push(CapabilityProviderConfig {
            id: "Trusted Runtime.Provider".into(),
            ..CapabilityProviderConfig::default()
        });
    assert!(matches!(
        capability_provider_registry(&duplicate_config),
        Err(CapabilityProviderRegistryError::DuplicateId { .. })
    ));
    assert!(!is_capability_provider_trusted_enabled(
        &duplicate_config,
        "trusted runtime.provider"
    ));
    let duplicate_diagnostics = capability_provider_diagnostics(&duplicate_config);
    assert_eq!(duplicate_diagnostics.total_providers, 3);
    assert_eq!(duplicate_diagnostics.registry_errors.len(), 1);

    denials::record(" coverage.tool ", "", "", "");
    denials::record(
        "secret.tool",
        "generated",
        "execute",
        "blocked Bearer super-secret-token",
    );
    let recent_denials = denials::list(2);
    assert_eq!(recent_denials[0].tool_name, "secret.tool");
    assert_eq!(recent_denials[0].reason, "[redacted: sensitive content]");
    assert_eq!(recent_denials[1].policy, "unknown");
    assert_eq!(recent_denials[1].action, "blocked");
    assert_eq!(recent_denials[1].reason, "<empty>");
    denials::record("  ", "policy", "deny", "ignored because tool name is blank");
    assert_eq!(denials::list(1)[0].tool_name, "secret.tool");
    denials::record("long.tool", "policy", "deny", &"a".repeat(10_000));
    let long_denial = denials::list(1).into_iter().next().expect("long denial");
    assert_eq!(long_denial.tool_name, "long.tool");
    assert!(long_denial.reason.ends_with('…'));
    assert!(long_denial.reason.chars().count() <= 241);

    let diagnostics = diagnostics_for_config(&config).value;
    assert!(diagnostics.total_tools > 0);
    assert!(diagnostics.enabled_tools > 0);
    assert!(diagnostics
        .policy_surfaces
        .iter()
        .any(|surface| surface == "approval.decide"));
    assert!(diagnostics
        .recent_denials
        .iter()
        .any(|denial| denial.tool_name == "secret.tool"));
    assert_eq!(diagnostics.capability_providers.total_providers, 2);
}

#[tokio::test]
async fn generated_tools_raw_paths_cover_admission_validation_and_execution() {
    let schema = json!({
        "type": "object",
        "properties": {
            "message": { "type": "string" }
        },
        "required": ["message"]
    });
    let mut definition = GeneratedToolDefinition::new(
        " generated.echo ",
        " Execute through the generated adapter. ",
        schema.clone(),
        " echo-generated ",
    );
    definition.permission_level = PermissionLevel::Write;
    definition.category = ToolCategory::Skill;
    definition.scope = ToolScope::All;
    definition.provider_id = Some(" Trusted.Runtime ".into());
    definition.capability_id = Some(" messages.send ".into());
    definition.source_digest = Some(" sha256:generated ".into());
    definition.risk = Some(GeneratedToolRisk::ExternalWrite);
    definition.policy_surface = Some(" generated.surface ".into());

    let admission = GeneratedToolAdmissionConfig {
        enforce_provenance: true,
        trusted_providers: BTreeSet::from(["TRUSTED.RUNTIME".to_string(), "bad/provider".into()]),
        disabled_providers: BTreeSet::from(["ignored/provider".into()]),
        existing_tool_names: BTreeSet::from(["reserved.tool".to_string()]),
        ..Default::default()
    };
    let report = admit_generated_tool_definitions(vec![definition.clone()], &admission);
    assert_eq!(report.rejected, Vec::new());
    assert_eq!(report.admitted.len(), 1);
    let admitted = report.admitted[0].clone();
    assert_eq!(admitted.name, "generated.echo");
    assert_eq!(
        admitted.description,
        "Execute through the generated adapter."
    );
    assert_eq!(admitted.adapter_id, "echo-generated");
    assert_eq!(admitted.provider_id.as_deref(), Some("trusted.runtime"));
    assert_eq!(admitted.capability_id.as_deref(), Some("messages.send"));
    assert_eq!(admitted.source_digest.as_deref(), Some("sha256:generated"));
    assert_eq!(
        admitted.policy_surface.as_deref(),
        Some("generated.surface")
    );

    let adapter = Arc::new(EchoGeneratedAdapter);
    let tools = generated_tools_from_definitions(vec![admitted.clone()], adapter.clone())
        .expect("generated tools should instantiate");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name(), "generated.echo");
    assert_eq!(
        tools[0].description(),
        "Execute through the generated adapter."
    );
    assert_eq!(tools[0].permission_level(), PermissionLevel::Write);
    assert_eq!(tools[0].category(), ToolCategory::Skill);
    assert_eq!(tools[0].scope(), ToolScope::All);
    assert_eq!(tools[0].parameters_schema(), schema);
    assert!(tools[0].external_effect());
    let result = tools[0]
        .execute(json!({ "message": "hello" }))
        .await
        .expect("generated tool execution");
    assert!(result.output().contains("generated.echo"));
    assert!(result.output().contains("hello"));

    let mut duplicate = admitted.clone();
    duplicate.name = "reserved.tool".into();
    let duplicate_report = admit_generated_tool_definitions(vec![duplicate], &admission);
    assert!(duplicate_report.admitted.is_empty());
    assert!(duplicate_report.rejected[0].reason.contains("duplicate"));

    let mut unsafe_name = admitted.clone();
    unsafe_name.name = "Bad Tool".into();
    let unsafe_report = admit_generated_tool_definitions(vec![unsafe_name], &admission);
    assert!(unsafe_report.admitted.is_empty());
    assert!(unsafe_report.rejected[0]
        .reason
        .contains("unsupported characters"));

    let mut missing_provider = admitted.clone();
    missing_provider.provider_id = None;
    let missing_provider_report =
        admit_generated_tool_definitions(vec![missing_provider], &admission);
    assert!(missing_provider_report.admitted.is_empty());
    assert!(missing_provider_report.rejected[0]
        .reason
        .contains("missing provider_id"));

    let mut bad_schema = admitted.clone();
    bad_schema.parameters_schema = json!({ "properties": {} });
    let bad_schema_report = admit_generated_tool_definitions(vec![bad_schema], &admission);
    assert!(bad_schema_report.admitted.is_empty());
    assert!(bad_schema_report.rejected[0]
        .reason
        .contains("invalid schema"));

    let mut adapter_mismatch = admitted.clone();
    adapter_mismatch.adapter_id = "other-adapter".into();
    match generated_tools_from_definitions(vec![adapter_mismatch], adapter) {
        Ok(_) => panic!("adapter mismatch should fail"),
        Err(error) => assert!(error.to_string().contains("requires adapter")),
    }
}
