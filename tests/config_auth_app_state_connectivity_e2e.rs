//! Focused JSON-RPC E2E coverage for config, auth/credentials, app_state,
//! and connectivity controller surfaces.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::http::header::AUTHORIZATION;
use reqwest::StatusCode;
use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};

use openhuman_core::core::auth::{init_rpc_token, CORE_TOKEN_ENV_VAR};
use openhuman_core::core::jsonrpc::build_core_http_router;

const TEST_RPC_TOKEN: &str = "worker-a-domain-e2e-token";

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

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    let mutex = ENV_LOCK.get_or_init(|| Mutex::new(()));
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn ensure_rpc_auth() {
    AUTH_INIT.get_or_init(|| {
        std::env::set_var(CORE_TOKEN_ENV_VAR, TEST_RPC_TOKEN);
        let token_dir = std::env::temp_dir().join("openhuman-worker-a-e2e-auth");
        init_rpc_token(&token_dir).expect("init rpc auth token");
    });
}

async fn serve_rpc() -> (
    SocketAddr,
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

fn write_min_config(openhuman_dir: &Path) {
    std::fs::create_dir_all(openhuman_dir).expect("create .openhuman");
    let cfg = r#"api_url = "http://127.0.0.1:9"
default_model = "worker-a-model"
default_temperature = 0.2

[secrets]
encrypt = false

[local_ai]
enabled = false
runtime_enabled = false
opt_in_confirmed = false

[memory]
provider = "none"
embedding_provider = "none"
embedding_model = "none"
embedding_dimensions = 0
auto_save = false

[memory_tree]
embedding_strict = false
"#;
    std::fs::write(openhuman_dir.join("config.toml"), cfg).expect("write config.toml");
    let _: openhuman_core::openhuman::config::Config =
        toml::from_str(cfg).expect("test config must match schema");
}

struct TestHarness {
    _tmp: TempDir,
    home: std::path::PathBuf,
    _guards: Vec<EnvVarGuard>,
    rpc_base: String,
    join: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

async fn setup() -> TestHarness {
    let tmp = tempdir().expect("tempdir");
    let home = tmp.path().to_path_buf();
    let openhuman_home = home.join(".openhuman");
    write_min_config(&openhuman_home);

    let guards = vec![
        EnvVarGuard::set_to_path("HOME", &home),
        EnvVarGuard::unset("OPENHUMAN_WORKSPACE"),
        EnvVarGuard::unset("BACKEND_URL"),
        EnvVarGuard::unset("VITE_BACKEND_URL"),
        EnvVarGuard::unset("OPENHUMAN_API_URL"),
        EnvVarGuard::unset("OPENHUMAN_CORE_RPC_URL"),
        EnvVarGuard::unset("OPENHUMAN_CORE_PORT"),
        EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file"),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_STRICT", "false"),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_ENDPOINT", ""),
        EnvVarGuard::set("OPENHUMAN_MEMORY_EMBED_MODEL", ""),
        EnvVarGuard::set("OPENHUMAN_BROWSER_ALLOW_ALL_RPC_ENABLE", ""),
    ];

    let (addr, join) = serve_rpc().await;
    TestHarness {
        _tmp: tmp,
        home,
        _guards: guards,
        rpc_base: format!("http://{addr}"),
        join,
    }
}

async fn schema(rpc_base: &str) -> Value {
    let url = format!("{}/schema", rpc_base.trim_end_matches('/'));
    reqwest::get(&url)
        .await
        .unwrap_or_else(|err| panic!("GET {url}: {err}"))
        .json::<Value>()
        .await
        .expect("schema json")
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

fn err<'a>(value: &'a Value, context: &str) -> &'a Value {
    value
        .get("error")
        .unwrap_or_else(|| panic!("{context}: expected JSON-RPC error, got: {value}"))
}

fn payload<'a>(value: &'a Value, context: &str) -> &'a Value {
    let result = ok(value, context);
    result.get("result").unwrap_or(result)
}

fn assert_error_contains(value: &Value, context: &str, needle: &str) {
    let message = err(value, context)
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        message.contains(needle),
        "{context}: expected error containing {needle:?}, got {message:?}"
    );
}

fn schema_method_names(value: &Value, namespace: &str) -> Vec<String> {
    let mut methods = value
        .get("methods")
        .and_then(Value::as_array)
        .expect("schema methods array")
        .iter()
        .filter(|method| method.get("namespace").and_then(Value::as_str) == Some(namespace))
        .map(|method| {
            method
                .get("method")
                .and_then(Value::as_str)
                .expect("method name")
                .to_string()
        })
        .collect::<Vec<_>>();
    methods.sort();
    methods
}

#[tokio::test]
async fn worker_a_controller_schemas_are_fully_exposed() {
    let _lock = env_lock();
    let harness = setup().await;

    let schema = schema(&harness.rpc_base).await;

    for (namespace, expected) in [
        (
            "config",
            vec![
                "openhuman.config_agent_server_status",
                "openhuman.config_get",
                "openhuman.config_get_analytics_settings",
                "openhuman.config_get_autonomy_settings",
                "openhuman.config_get_client_config",
                "openhuman.config_get_composio_trigger_settings",
                "openhuman.config_get_dashboard_settings",
                "openhuman.config_get_data_paths",
                "openhuman.config_get_dictation_settings",
                "openhuman.config_get_meet_settings",
                "openhuman.config_get_onboarding_completed",
                "openhuman.config_get_runtime_flags",
                "openhuman.config_get_search_settings",
                "openhuman.config_get_voice_server_settings",
                "openhuman.config_reset_local_data",
                "openhuman.config_resolve_api_url",
                "openhuman.config_set_browser_allow_all",
                "openhuman.config_set_onboarding_completed",
                "openhuman.config_update_analytics_settings",
                "openhuman.config_update_autonomy_settings",
                "openhuman.config_update_browser_settings",
                "openhuman.config_update_composio_trigger_settings",
                "openhuman.config_update_dictation_settings",
                "openhuman.config_update_local_ai_settings",
                "openhuman.config_update_meet_settings",
                "openhuman.config_update_memory_settings",
                "openhuman.config_update_model_settings",
                "openhuman.config_update_runtime_settings",
                "openhuman.config_update_screen_intelligence_settings",
                "openhuman.config_update_search_settings",
                "openhuman.config_update_voice_server_settings",
                "openhuman.config_workspace_onboarding_flag_exists",
                "openhuman.config_workspace_onboarding_flag_set",
            ],
        ),
        (
            "auth",
            vec![
                "openhuman.auth_clear_session",
                "openhuman.auth_consume_login_token",
                "openhuman.auth_create_channel_link_token",
                "openhuman.auth_get_me",
                "openhuman.auth_get_session_token",
                "openhuman.auth_get_state",
                "openhuman.auth_list_provider_credentials",
                "openhuman.auth_oauth_connect",
                "openhuman.auth_oauth_fetch_client_key",
                "openhuman.auth_oauth_fetch_integration_tokens",
                "openhuman.auth_oauth_list_integrations",
                "openhuman.auth_oauth_revoke_integration",
                "openhuman.auth_remove_provider_credentials",
                "openhuman.auth_store_provider_credentials",
                "openhuman.auth_store_session",
            ],
        ),
        (
            "app_state",
            vec![
                "openhuman.app_state_snapshot",
                "openhuman.app_state_update_local_state",
            ],
        ),
        ("connectivity", vec!["openhuman.connectivity_diag"]),
    ] {
        assert_eq!(
            schema_method_names(&schema, namespace),
            expected,
            "schema catalog mismatch for namespace {namespace}"
        );
    }

    harness.join.abort();
}

#[tokio::test]
async fn config_controller_mutations_round_trip_over_json_rpc() {
    let _lock = env_lock();
    let harness = setup().await;

    let initial = rpc(&harness.rpc_base, 10_001, "openhuman.config_get", json!({})).await;
    assert!(
        payload(&initial, "config_get")
            .get("workspace_dir")
            .and_then(Value::as_str)
            .is_some(),
        "config_get should expose resolved paths: {initial}"
    );

    let model = rpc(
        &harness.rpc_base,
        10_002,
        "openhuman.config_update_model_settings",
        json!({
            "api_url": "http://127.0.0.1:9",
            "inference_url": "http://127.0.0.1:19999/v1",
            "api_key": "worker-a-secret",
            "default_model": "worker-a-updated",
            "default_temperature": 0.4,
            "model_routes": [{ "hint": "reasoning", "model": "route-model" }],
            "cloud_providers": [{
                "id": "provider-a",
                "slug": "worker-a-cloud",
                "label": "Worker A Cloud",
                "endpoint": "http://127.0.0.1:19999/v1",
                "auth_style": "bearer"
            }],
            "primary_cloud": "provider-a",
            "chat_provider": "worker-a-cloud:chat",
            "reasoning_provider": "worker-a-cloud:reason",
            "agentic_provider": "worker-a-cloud:agent",
            "coding_provider": "worker-a-cloud:code",
            "memory_provider": "worker-a-cloud:memory",
            "embeddings_provider": "worker-a-cloud:embeddings",
            "heartbeat_provider": "worker-a-cloud:heartbeat",
            "learning_provider": "worker-a-cloud:learning",
            "subconscious_provider": "worker-a-cloud:subconscious"
        }),
    )
    .await;
    ok(&model, "update_model_settings");

    let client = rpc(
        &harness.rpc_base,
        10_003,
        "openhuman.config_get_client_config",
        json!({}),
    )
    .await;
    let client_payload = payload(&client, "get_client_config");
    assert_eq!(
        client_payload.get("default_model").and_then(Value::as_str),
        Some("worker-a-updated")
    );
    assert_eq!(
        client_payload.get("api_key_set").and_then(Value::as_bool),
        Some(true),
        "client config should expose only API key presence: {client_payload}"
    );
    assert!(
        !client_payload.to_string().contains("worker-a-secret"),
        "client config must not echo local API keys: {client_payload}"
    );

    let memory = rpc(
        &harness.rpc_base,
        10_004,
        "openhuman.config_update_memory_settings",
        json!({
            "backend": "sqlite",
            "auto_save": true,
            "embedding_provider": "none",
            "embedding_model": "none",
            "embedding_dimensions": 0,
            "memory_window": "minimal"
        }),
    )
    .await;
    ok(&memory, "update_memory_settings");

    for (id, method, params) in [
        (
            10_005,
            "openhuman.config_update_screen_intelligence_settings",
            json!({
                "enabled": false,
                "capture_policy": "off",
                "baseline_fps": 0.5,
                "vision_enabled": false,
                "autocomplete_enabled": false,
                "use_vision_model": false,
                "keep_screenshots": false,
                "allowlist": ["Finder"],
                "denylist": ["Passwords"]
            }),
        ),
        (
            10_006,
            "openhuman.config_update_runtime_settings",
            json!({ "kind": "local", "reasoning_enabled": true }),
        ),
        (
            10_007,
            "openhuman.config_update_browser_settings",
            json!({ "enabled": true }),
        ),
        (
            10_008,
            "openhuman.config_update_local_ai_settings",
            json!({
                "runtime_enabled": false,
                "opt_in_confirmed": false,
                "provider": "ollama",
                "base_url": "http://127.0.0.1:11434",
                "model_id": "llama3",
                "chat_model_id": "llama3",
                "usage_embeddings": false,
                "usage_heartbeat": false,
                "usage_learning_reflection": false,
                "usage_subconscious": false
            }),
        ),
        (
            10_009,
            "openhuman.config_update_voice_server_settings",
            json!({
                "auto_start": false,
                "hotkey": "Fn",
                "activation_mode": "push",
                "skip_cleanup": true,
                "min_duration_secs": 0.25,
                "silence_threshold": 0.01,
                "custom_dictionary": ["OpenHuman", "WorkerA"]
            }),
        ),
        (
            10_010,
            "openhuman.config_update_composio_trigger_settings",
            json!({
                "triage_disabled": true,
                "triage_disabled_toolkits": ["gmail", "slack"]
            }),
        ),
        (
            10_011,
            "openhuman.config_update_autonomy_settings",
            json!({
                "level": "supervised",
                "workspace_only": true,
                "allowed_commands": ["git", "cargo"],
                "forbidden_paths": ["/tmp/forbidden-worker-a"],
                "trusted_roots": [{
                    "path": harness.home.display().to_string(),
                    "access": "read"
                }],
                "allow_tool_install": false,
                "max_actions_per_hour": 42,
                "auto_approve": ["memory.search"],
                "require_task_plan_approval": true
            }),
        ),
        (
            10_012,
            "openhuman.config_update_search_settings",
            json!({
                "engine": "managed",
                "max_results": 5,
                "timeout_secs": 12,
                "parallel_api_key": "parallel-secret",
                "brave_api_key": "brave-secret",
                "querit_api_key": "querit-secret",
                "allowed_domains": ["example.com"],
                "allow_all": false
            }),
        ),
    ] {
        let response = rpc(&harness.rpc_base, id, method, params).await;
        ok(&response, method);
    }

    for (id, method) in [
        (10_101, "openhuman.config_resolve_api_url"),
        (10_102, "openhuman.config_get_runtime_flags"),
        (10_103, "openhuman.config_get_dashboard_settings"),
        (10_104, "openhuman.config_agent_server_status"),
        (10_105, "openhuman.config_get_data_paths"),
        (10_106, "openhuman.config_get_voice_server_settings"),
        (10_107, "openhuman.config_get_composio_trigger_settings"),
        (10_108, "openhuman.config_get_autonomy_settings"),
        (10_109, "openhuman.config_get_search_settings"),
    ] {
        let response = rpc(&harness.rpc_base, id, method, json!({})).await;
        ok(&response, method);
    }

    let allow_all = rpc(
        &harness.rpc_base,
        10_201,
        "openhuman.config_set_browser_allow_all",
        json!({ "enabled": false }),
    )
    .await;
    ok(&allow_all, "set_browser_allow_all false");

    let exists_before = rpc(
        &harness.rpc_base,
        10_202,
        "openhuman.config_workspace_onboarding_flag_exists",
        json!({ "flag_name": ".worker-a-onboarding" }),
    )
    .await;
    assert_eq!(
        payload(&exists_before, "workspace_onboarding_flag_exists before").as_bool(),
        Some(false)
    );

    let set_flag = rpc(
        &harness.rpc_base,
        10_203,
        "openhuman.config_workspace_onboarding_flag_set",
        json!({ "flag_name": ".worker-a-onboarding", "value": true }),
    )
    .await;
    assert_eq!(
        payload(&set_flag, "workspace_onboarding_flag_set true").as_bool(),
        Some(true)
    );

    let clear_flag = rpc(
        &harness.rpc_base,
        10_204,
        "openhuman.config_workspace_onboarding_flag_set",
        json!({ "flag_name": ".worker-a-onboarding", "value": false }),
    )
    .await;
    assert_eq!(
        payload(&clear_flag, "workspace_onboarding_flag_set false").as_bool(),
        Some(false)
    );

    let reset = rpc(
        &harness.rpc_base,
        10_301,
        "openhuman.config_reset_local_data",
        json!({}),
    )
    .await;
    assert!(
        payload(&reset, "reset_local_data").is_object(),
        "reset should return a result payload: {reset}"
    );

    harness.join.abort();
}

#[tokio::test]
async fn auth_credentials_controller_paths_round_trip_and_validate_errors() {
    let _lock = env_lock();
    let harness = setup().await;

    let state = rpc(
        &harness.rpc_base,
        20_001,
        "openhuman.auth_get_state",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&state, "auth_get_state")
            .get("isAuthenticated")
            .and_then(Value::as_bool),
        Some(false)
    );

    let token = rpc(
        &harness.rpc_base,
        20_002,
        "openhuman.auth_get_session_token",
        json!({}),
    )
    .await;
    assert!(
        payload(&token, "auth_get_session_token")
            .get("token")
            .is_some(),
        "session token read should return a token field even when empty: {token}"
    );

    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_003,
            "openhuman.auth_get_me",
            json!({}),
        )
        .await,
        "auth_get_me without session",
        "session JWT required",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_004,
            "openhuman.auth_consume_login_token",
            json!({ "loginToken": "" }),
        )
        .await,
        "auth_consume_login_token empty",
        "loginToken is required",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_005,
            "openhuman.auth_create_channel_link_token",
            json!({ "channel": "mastodon" }),
        )
        .await,
        "auth_create_channel_link_token unsupported",
        "unsupported channel",
    );

    for (id, method, params, needle) in [
        (
            20_006,
            "openhuman.auth_oauth_connect",
            json!({ "provider": "github" }),
            "session JWT required",
        ),
        (
            20_007,
            "openhuman.auth_oauth_list_integrations",
            json!({}),
            "session JWT required",
        ),
        (
            20_008,
            "openhuman.auth_oauth_fetch_integration_tokens",
            json!({ "integrationId": "abc", "key": "secret" }),
            "session JWT required",
        ),
        (
            20_009,
            "openhuman.auth_oauth_fetch_client_key",
            json!({ "integrationId": "abc" }),
            "session JWT required",
        ),
        (
            20_010,
            "openhuman.auth_oauth_revoke_integration",
            json!({ "integrationId": "abc" }),
            "session JWT required",
        ),
    ] {
        let response = rpc(&harness.rpc_base, id, method, params).await;
        assert_error_contains(&response, method, needle);
    }

    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_011,
            "openhuman.auth_store_provider_credentials",
            json!({ "provider": "   " }),
        )
        .await,
        "auth_store_provider_credentials empty provider",
        "provider is required",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_012,
            "openhuman.auth_store_provider_credentials",
            json!({ "provider": "worker-a", "fields": "not-an-object" }),
        )
        .await,
        "auth_store_provider_credentials invalid fields",
        "fields must be a JSON object",
    );

    let stored_provider = rpc(
        &harness.rpc_base,
        20_013,
        "openhuman.auth_store_provider_credentials",
        json!({
            "provider": "worker-a",
            "profile": "secondary",
            "token": "provider-secret",
            "fields": { "region": "test" },
            "setActive": false
        }),
    )
    .await;
    assert_eq!(
        payload(&stored_provider, "auth_store_provider_credentials")
            .get("provider")
            .and_then(Value::as_str),
        Some("worker-a")
    );

    let listed = rpc(
        &harness.rpc_base,
        20_014,
        "openhuman.auth_list_provider_credentials",
        json!({ "provider": "worker-a" }),
    )
    .await;
    let listed_payload = payload(&listed, "auth_list_provider_credentials");
    assert!(
        listed_payload
            .as_array()
            .expect("credentials list")
            .iter()
            .any(|profile| profile.get("profileName").and_then(Value::as_str) == Some("secondary")),
        "stored provider profile should be listed: {listed_payload}"
    );

    let removed = rpc(
        &harness.rpc_base,
        20_015,
        "openhuman.auth_remove_provider_credentials",
        json!({ "provider": "worker-a", "profile": "secondary" }),
    )
    .await;
    assert_eq!(
        payload(&removed, "auth_remove_provider_credentials")
            .get("removed")
            .and_then(Value::as_bool),
        Some(true)
    );

    let session = rpc(
        &harness.rpc_base,
        20_016,
        "openhuman.auth_store_session",
        json!({
            "token": "header.payload.local",
            "user": {
                "id": "ignored",
                "name": "Worker A",
                "email": "worker-a@example.test"
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&session, "auth_store_session")
            .get("provider")
            .and_then(Value::as_str),
        Some("app-session")
    );

    let authed = rpc(
        &harness.rpc_base,
        20_017,
        "openhuman.auth_get_state",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&authed, "auth_get_state")
            .get("isAuthenticated")
            .and_then(Value::as_bool),
        Some(true)
    );

    let cleared = rpc(
        &harness.rpc_base,
        20_018,
        "openhuman.auth_clear_session",
        json!({}),
    )
    .await;
    assert!(
        payload(&cleared, "auth_clear_session")
            .get("removed")
            .and_then(Value::as_bool)
            .is_some(),
        "clear session should return removal status: {cleared}"
    );

    harness.join.abort();
}

#[tokio::test]
async fn app_state_update_persists_and_snapshot_reads_local_state() {
    let _lock = env_lock();
    let harness = setup().await;

    let updated = rpc(
        &harness.rpc_base,
        30_001,
        "openhuman.app_state_update_local_state",
        json!({
            "encryptionKey": "worker-a-key",
            "onboardingTasks": {
                "accessibilityPermissionGranted": true,
                "localModelConsentGiven": true,
                "localModelDownloadStarted": false,
                "enabledTools": ["memory.search", "tools.web_search"],
                "connectedSources": ["gmail"],
                "updatedAtMs": 123456
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&updated, "app_state_update_local_state")
            .get("encryptionKey")
            .and_then(Value::as_str),
        Some("worker-a-key")
    );

    let snapshot = rpc(
        &harness.rpc_base,
        30_002,
        "openhuman.app_state_snapshot",
        json!({}),
    )
    .await;
    let local_state = payload(&snapshot, "app_state_snapshot")
        .get("localState")
        .unwrap_or_else(|| panic!("snapshot should include localState: {snapshot}"));
    assert_eq!(
        local_state.get("encryptionKey").and_then(Value::as_str),
        Some("worker-a-key")
    );
    assert_eq!(
        local_state.pointer("/onboardingTasks/enabledTools/0"),
        Some(&json!("memory.search"))
    );

    let cleared = rpc(
        &harness.rpc_base,
        30_003,
        "openhuman.app_state_update_local_state",
        json!({
            "encryptionKey": null,
            "onboardingTasks": null
        }),
    )
    .await;
    assert!(
        payload(&cleared, "app_state_update_local_state")
            .get("encryptionKey")
            .is_none(),
        "null patch should clear optional encryption key: {cleared}"
    );

    harness.join.abort();
}

#[tokio::test]
async fn connectivity_diag_reports_runtime_port_sources() {
    let _lock = env_lock();
    let harness = setup().await;

    let diag = rpc(
        &harness.rpc_base,
        40_001,
        "openhuman.connectivity_diag",
        json!({}),
    )
    .await;
    let diag_payload = payload(&diag, "connectivity_diag")
        .get("diag")
        .unwrap_or_else(|| panic!("connectivity diag should include diag payload: {diag}"));
    assert!(
        diag_payload
            .get("sidecar_pid")
            .and_then(Value::as_u64)
            .is_some(),
        "diag should expose sidecar_pid: {diag_payload}"
    );
    assert!(
        diag_payload
            .get("listen_port")
            .and_then(Value::as_u64)
            .is_some(),
        "diag should expose listen_port: {diag_payload}"
    );
    assert!(
        diag_payload
            .get("listen_port_in_use")
            .and_then(Value::as_bool)
            .is_some(),
        "diag should expose listen_port_in_use: {diag_payload}"
    );

    harness.join.abort();
}
