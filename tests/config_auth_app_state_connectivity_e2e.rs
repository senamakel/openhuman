//! Focused JSON-RPC E2E coverage for config, auth/credentials, app_state,
//! and connectivity controller surfaces.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;

use axum::extract::{Path as AxumPath, State};
use axum::http::{header::AUTHORIZATION, HeaderMap};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};

use openhuman_core::api::config::{
    api_base_from_env, api_url, app_env_from_env, default_api_base_url_for_env, effective_api_url,
    effective_backend_api_url, effective_inference_url, looks_like_local_ai_endpoint,
    normalize_api_base_url, APP_ENV_VAR, DEFAULT_API_BASE_URL, DEFAULT_STAGING_API_BASE_URL,
    OPENHUMAN_INFERENCE_PATH, VITE_APP_ENV_VAR,
};
use openhuman_core::core::auth::{init_rpc_token, CORE_TOKEN_ENV_VAR};
use openhuman_core::core::jsonrpc::build_core_http_router;
use openhuman_core::openhuman::app_state::app_state_schemas;
use openhuman_core::openhuman::config::schema::{
    generate_provider_id, generate_voice_provider_id, is_slug_reserved, is_voice_slug_reserved,
    migrate_legacy_fields, AuthStyle, CloudProviderCreds, CloudProviderType, MemoryContextWindow,
    ProxyConfig, ProxyScope, VoiceCapability, VoiceProviderCreds, WhatsAppConfig,
};
use openhuman_core::openhuman::config::{AgentConfig, ChannelsConfig, Config, TeamModelConfig};
use openhuman_core::openhuman::credentials::cli::{
    cli_auth_list, cli_auth_login, cli_auth_logout, cli_auth_status, parse_field_equals_entries,
};
use openhuman_core::openhuman::credentials::profiles::{AuthProfile, AuthProfilesStore, TokenSet};
use openhuman_core::openhuman::credentials::{
    clear_composio_api_key, get_composio_api_key, normalize_provider, rpc_store_composio_api_key,
    store_composio_api_key, AuthService, APP_SESSION_PROVIDER, COMPOSIO_DIRECT_PROVIDER,
};

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

#[derive(Clone, Default)]
struct MockBackendState {
    auth_me_hits: Arc<AtomicUsize>,
}

async fn serve_mock_backend() -> (
    String,
    MockBackendState,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    let state = MockBackendState::default();
    let app = Router::new()
        .route("/auth/me", get(mock_auth_me))
        .route(
            "/telegram/login-tokens/{token}/consume",
            post(mock_consume_login_token),
        )
        .route(
            "/auth/channels/{channel}/link-token",
            post(mock_channel_link_token),
        )
        .route("/auth/integrations", get(mock_integrations))
        .route(
            "/auth/integrations/{integration_id}/client-key",
            post(mock_client_key),
        )
        .route(
            "/auth/integrations/{integration_id}",
            delete(mock_revoke_integration),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock backend");
    let addr = listener.local_addr().expect("mock backend addr");
    let join = tokio::spawn(async move { axum::serve(listener, app).await });
    (format!("http://{addr}"), state, join)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
}

async fn mock_auth_me(State(state): State<MockBackendState>, headers: HeaderMap) -> Json<Value> {
    state.auth_me_hits.fetch_add(1, Ordering::SeqCst);
    let auth = bearer(&headers).unwrap_or_default();
    Json(json!({
        "success": true,
        "data": {
            "id": "remote-user-1",
            "_id": "remote-user-1",
            "name": "Remote Worker",
            "email": "remote-worker@example.test",
            "authHeader": auth
        }
    }))
}

async fn mock_consume_login_token(AxumPath(token): AxumPath<String>) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "jwtToken": format!("jwt-from-{token}")
        }
    }))
}

async fn mock_channel_link_token(AxumPath(channel): AxumPath<String>) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "channel": channel,
            "linkToken": "link-token-123",
            "expiresIn": 300
        }
    }))
}

async fn mock_integrations() -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "integrations": [{
                "id": "0123456789abcdef01234567",
                "provider": "github",
                "createdAt": "2026-01-01T00:00:00Z"
            }]
        }
    }))
}

async fn mock_client_key(AxumPath(integration_id): AxumPath<String>) -> Json<Value> {
    Json(json!({
        "success": true,
        "data": {
            "integrationId": integration_id,
            "clientKey": "client-key-share"
        }
    }))
}

async fn mock_revoke_integration(AxumPath(_integration_id): AxumPath<String>) -> Json<Value> {
    Json(json!({ "success": true, "data": { "revoked": true } }))
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

#[test]
fn config_schema_helpers_cover_provider_voice_agent_and_channel_defaults() {
    let mut provider = CloudProviderCreds {
        id: "provider-legacy".to_string(),
        legacy_type: Some("anthropic".to_string()),
        ..CloudProviderCreds::default()
    };
    migrate_legacy_fields(&mut provider);
    assert_eq!(provider.slug, "anthropic");
    assert_eq!(provider.label, "Anthropic");
    assert_eq!(provider.endpoint, "https://api.anthropic.com/v1");
    assert_eq!(provider.auth_style, AuthStyle::Anthropic);
    assert_eq!(AuthStyle::OpenhumanJwt.as_str(), "openhuman_jwt");
    assert_eq!(
        CloudProviderType::Openrouter.default_endpoint(),
        "https://openrouter.ai/api/v1"
    );
    assert_eq!(CloudProviderType::Orcarouter.label(), "OrcaRouter");
    assert_eq!(CloudProviderType::Openhuman.as_str(), "openhuman");
    assert_eq!(
        CloudProviderType::Anthropic.auth_style(),
        AuthStyle::Anthropic
    );
    assert!(is_slug_reserved(" cloud "));
    assert!(!is_slug_reserved("ollama"));

    let provider_id = generate_provider_id("my provider!");
    assert!(provider_id.starts_with("p_my_provider__"));
    assert_eq!(provider_id.rsplit('_').next().unwrap().len(), 5);

    assert!(VoiceCapability::Stt.supports_stt());
    assert!(!VoiceCapability::Stt.supports_tts());
    assert_eq!(VoiceCapability::Tts.as_str(), "tts");
    assert!(VoiceCapability::Both.supports_stt());
    assert!(VoiceCapability::Both.supports_tts());
    assert_eq!(VoiceProviderCreds::default().auth_style, AuthStyle::Bearer);
    assert_eq!(
        openhuman_core::openhuman::config::schema::voice_providers::builtin_voice_provider(
            "deepgram"
        )
        .expect("deepgram builtin")
        .default_stt_model,
        Some("nova-2")
    );
    assert!(is_voice_slug_reserved(" whisper "));
    assert!(!is_voice_slug_reserved("openai"));
    let voice_id = generate_voice_provider_id("voice provider!");
    assert!(voice_id.starts_with("vp_voice_provider__"));
    assert_eq!(voice_id.rsplit('_').next().unwrap().len(), 5);

    let team = TeamModelConfig {
        lead_model: Some(" lead-model ".to_string()),
        agent_model: None,
    };
    assert_eq!(team.model_for_role(true), Some("lead-model"));
    assert_eq!(team.model_for_role(false), Some("lead-model"));
    assert_eq!(
        MemoryContextWindow::from_str_opt("MAXIMUM"),
        Some(MemoryContextWindow::Maximum)
    );
    assert_eq!(MemoryContextWindow::Extended.as_str(), "extended");
    let mut agent = AgentConfig {
        max_memory_context_chars: 20_000,
        ..AgentConfig::default()
    };
    assert_eq!(
        agent.resolved_memory_limits().max_memory_context_chars,
        MemoryContextWindow::Maximum
            .limits()
            .max_memory_context_chars
    );
    agent.memory_window = Some(MemoryContextWindow::Minimal);
    assert_eq!(
        agent.resolved_memory_limits(),
        MemoryContextWindow::Minimal.limits()
    );

    let default_channels = ChannelsConfig::default();
    assert!(!default_channels.has_listening_integrations());
    let mut listening_channels = default_channels.clone();
    listening_channels.whatsapp = Some(WhatsAppConfig {
        access_token: Some("token".to_string()),
        phone_number_id: Some("phone".to_string()),
        verify_token: Some("verify".to_string()),
        app_secret: None,
        session_path: None,
        pair_phone: None,
        pair_code: None,
        allowed_numbers: vec![],
    });
    assert!(listening_channels.has_listening_integrations());
    let whatsapp = listening_channels
        .whatsapp
        .as_ref()
        .expect("whatsapp config");
    assert_eq!(whatsapp.backend_type(), "cloud");
    assert!(whatsapp.is_cloud_config());
    assert!(!whatsapp.is_web_config());
}

#[test]
fn config_proxy_public_paths_normalize_validate_and_apply_scope() {
    let _lock = env_lock();
    let _http = EnvVarGuard::unset("HTTP_PROXY");
    let _https = EnvVarGuard::unset("HTTPS_PROXY");
    let _all = EnvVarGuard::unset("ALL_PROXY");
    let _no = EnvVarGuard::unset("NO_PROXY");
    let _http_lower = EnvVarGuard::unset("http_proxy");
    let _https_lower = EnvVarGuard::unset("https_proxy");
    let _all_lower = EnvVarGuard::unset("all_proxy");
    let _no_lower = EnvVarGuard::unset("no_proxy");

    assert!(ProxyConfig::supported_service_keys()
        .iter()
        .any(|key| *key == "memory.embeddings"));
    assert!(ProxyConfig::supported_service_selectors()
        .iter()
        .any(|selector| *selector == "tool.*"));

    let services = ProxyConfig {
        enabled: true,
        http_proxy: Some(" http://proxy.example:8080 ".into()),
        https_proxy: Some("https://secure-proxy.example".into()),
        all_proxy: None,
        no_proxy: vec![" localhost, 127.0.0.1 ".into(), "example.test".into()],
        scope: ProxyScope::Services,
        services: vec![
            " Tool.* ".into(),
            "tool.browser".into(),
            "memory.embeddings".into(),
        ],
    };
    services.validate().expect("valid services proxy");
    assert_eq!(
        services.normalized_services(),
        vec!["memory.embeddings", "tool.*", "tool.browser"]
    );
    assert_eq!(
        services.normalized_no_proxy(),
        vec!["127.0.0.1", "example.test", "localhost"]
    );
    assert!(services.should_apply_to_service("tool.http_request"));
    assert!(services.should_apply_to_service("memory.embeddings"));
    assert!(!services.should_apply_to_service("provider.openai"));
    assert!(!services.should_apply_to_service("   "));
    let _client = services
        .apply_to_reqwest_builder(reqwest::Client::builder(), "tool.browser")
        .build()
        .expect("proxied client builds");

    let env_scope = ProxyConfig {
        enabled: true,
        scope: ProxyScope::Environment,
        all_proxy: Some("socks5h://proxy.example:1080".into()),
        ..ProxyConfig::default()
    };
    env_scope.validate().expect("valid env proxy");
    assert!(!env_scope.should_apply_to_service("tool.browser"));
    env_scope.apply_to_process_env();
    assert_eq!(
        std::env::var("ALL_PROXY").as_deref(),
        Ok("socks5h://proxy.example:1080")
    );
    assert_eq!(
        std::env::var("all_proxy").as_deref(),
        Ok("socks5h://proxy.example:1080")
    );

    ProxyConfig::clear_process_env();
    assert!(std::env::var("ALL_PROXY").is_err());
    assert!(std::env::var("all_proxy").is_err());

    for mut invalid in [
        ProxyConfig {
            enabled: true,
            http_proxy: Some("ftp://proxy.example".into()),
            ..ProxyConfig::default()
        },
        ProxyConfig {
            enabled: true,
            scope: ProxyScope::Services,
            services: vec![],
            http_proxy: Some("http://proxy.example".into()),
            ..ProxyConfig::default()
        },
        ProxyConfig {
            enabled: true,
            http_proxy: None,
            https_proxy: None,
            all_proxy: None,
            ..ProxyConfig::default()
        },
        ProxyConfig {
            enabled: false,
            services: vec!["unknown.service".into()],
            ..ProxyConfig::default()
        },
    ] {
        assert!(
            invalid.validate().is_err(),
            "invalid proxy config should fail: {invalid:?}"
        );
        invalid.enabled = false;
    }

    openhuman_core::openhuman::config::set_runtime_proxy_config(services.clone());
    assert!(openhuman_core::openhuman::config::runtime_proxy_config()
        .should_apply_to_service("tool.browser"));
    let _cached = openhuman_core::openhuman::config::build_runtime_proxy_client("tool.browser");
    let _cached_again =
        openhuman_core::openhuman::config::build_runtime_proxy_client("tool.browser");
    let _timeout_client =
        openhuman_core::openhuman::config::build_runtime_proxy_client_with_timeouts(
            "memory.embeddings",
            1,
            1,
        );
    let _builder = openhuman_core::openhuman::config::apply_runtime_proxy_to_builder(
        reqwest::Client::builder(),
        "tool.http_request",
    );
    openhuman_core::openhuman::config::set_runtime_proxy_config(ProxyConfig::default());
}

#[test]
fn api_config_url_resolution_classifies_backend_and_inference_paths() {
    let _lock = env_lock();
    let _backend = EnvVarGuard::unset("BACKEND_URL");
    let _vite_backend = EnvVarGuard::unset("VITE_BACKEND_URL");
    let _app_env = EnvVarGuard::unset(APP_ENV_VAR);
    let _vite_app_env = EnvVarGuard::unset(VITE_APP_ENV_VAR);

    assert_eq!(
        normalize_api_base_url(" https://api.example.test/// "),
        "https://api.example.test"
    );
    assert_eq!(
        api_url(
            "https://api.tinyhumans.ai/openai/v1/chat/completions",
            "/auth/me"
        ),
        "https://api.tinyhumans.ai/auth/me"
    );
    assert_eq!(api_url("not a url/", "auth/me"), "not a url/auth/me");
    assert_eq!(
        api_url(" https://api.tinyhumans.ai/ ", ""),
        "https://api.tinyhumans.ai"
    );

    assert!(looks_like_local_ai_endpoint("http://localhost:11434"));
    assert!(looks_like_local_ai_endpoint(
        "http://10.0.0.2/v1/chat/completions"
    ));
    assert!(looks_like_local_ai_endpoint(
        "https://api.openai.com/v1/completions"
    ));
    assert!(!looks_like_local_ai_endpoint("http://127.0.0.1:45678"));
    assert!(!looks_like_local_ai_endpoint("not a url"));

    assert_eq!(
        effective_api_url(&Some(" http://127.0.0.1:11434/ ".into())),
        "http://127.0.0.1:11434"
    );
    assert_eq!(
        effective_inference_url(&Some("https://api.tinyhumans.ai".into()), &None),
        format!("https://api.tinyhumans.ai{OPENHUMAN_INFERENCE_PATH}")
    );
    assert_eq!(
        effective_inference_url(
            &Some("https://api.tinyhumans.ai".into()),
            &Some(" http://127.0.0.1:11434/v1/chat/completions ".into())
        ),
        "http://127.0.0.1:11434/v1/chat/completions"
    );

    assert_eq!(
        effective_backend_api_url(&Some(" http://127.0.0.1:11434/v1 ".into())),
        DEFAULT_API_BASE_URL
    );
    assert_eq!(
        effective_backend_api_url(&Some(
            " https://api.tinyhumans.ai/openai/v1/chat/completions?x=1#frag ".into()
        )),
        "https://api.tinyhumans.ai"
    );

    std::env::set_var("BACKEND_URL", "");
    std::env::set_var(
        "VITE_BACKEND_URL",
        " https://backend.example.test/openai/v1/chat/completions ",
    );
    assert_eq!(
        api_base_from_env().as_deref(),
        Some("https://backend.example.test/openai/v1/chat/completions")
    );
    assert_eq!(
        effective_backend_api_url(&None),
        "https://backend.example.test"
    );

    std::env::set_var(APP_ENV_VAR, " Staging ");
    assert_eq!(app_env_from_env().as_deref(), Some("staging"));
    assert_eq!(
        default_api_base_url_for_env(app_env_from_env().as_deref()),
        DEFAULT_STAGING_API_BASE_URL
    );
}

#[test]
fn auth_service_direct_paths_cover_profile_selection_and_validation() {
    let tmp = tempdir().expect("tempdir");
    let auth = AuthService::new(tmp.path(), false);

    assert_eq!(normalize_provider("  GitHub  ").unwrap(), "github");
    assert!(normalize_provider("   ").is_err());
    assert!(auth.set_active_profile("github", "missing").is_err());
    assert_eq!(
        auth.get_provider_bearer_token("github", None)
            .expect("missing profile lookup"),
        None
    );

    let stored = auth
        .store_provider_token(
            "github",
            "personal",
            " ghp-token ",
            [("scope".to_string(), "repo".to_string())]
                .into_iter()
                .collect(),
            false,
        )
        .expect("store token profile");
    assert_eq!(stored.provider, "github");
    assert_eq!(
        auth.get_provider_bearer_token("GitHub", Some("personal"))
            .expect("profile override lookup"),
        Some(" ghp-token ".to_string())
    );
    assert_eq!(
        auth.get_provider_bearer_token("GitHub", None)
            .expect("no active/default lookup"),
        Some(" ghp-token ".to_string())
    );

    let active_id = auth
        .set_active_profile("GitHub", "personal")
        .expect("set active by profile name");
    assert_eq!(active_id, stored.id);
    assert!(auth
        .remove_profile("GitHub", "personal")
        .expect("remove stored profile"));
    assert!(!auth
        .remove_profile("GitHub", "personal")
        .expect("remove missing profile"));
}

#[tokio::test]
async fn composio_direct_credentials_helpers_trim_store_and_clear_key() {
    let _lock = env_lock();
    let tmp = tempdir().expect("tempdir");
    let mut config = Config::default();
    config.config_path = tmp.path().join("config.toml");
    config.workspace_dir = tmp.path().join("workspace");
    config.secrets.encrypt = false;
    std::fs::create_dir_all(config.config_path.parent().expect("config parent"))
        .expect("create config parent");

    assert_eq!(
        get_composio_api_key(&config).expect("empty composio key store"),
        None
    );
    assert!(
        store_composio_api_key(&config, "   ").await.is_err(),
        "blank composio keys should be rejected"
    );

    let stored = store_composio_api_key(&config, "  cmp_worker_a_secret  ")
        .await
        .expect("store direct composio key");
    assert_eq!(
        stored.value.get("provider").and_then(Value::as_str),
        Some(COMPOSIO_DIRECT_PROVIDER)
    );
    assert_eq!(
        get_composio_api_key(&config).expect("stored composio key"),
        Some("cmp_worker_a_secret".to_string())
    );

    let stored_via_rpc = rpc_store_composio_api_key(&config, "cmp_worker_a_second")
        .await
        .expect("store direct composio key via rpc helper");
    assert_eq!(
        stored_via_rpc.value.get("stored").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        get_composio_api_key(&config).expect("updated composio key"),
        Some("cmp_worker_a_second".to_string())
    );

    let cleared = clear_composio_api_key(&config)
        .await
        .expect("clear direct composio key");
    assert_eq!(
        cleared.value.get("removed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        get_composio_api_key(&config).expect("cleared composio key"),
        None
    );
    let cleared_again = clear_composio_api_key(&config)
        .await
        .expect("clear missing direct composio key");
    assert_eq!(
        cleared_again.value.get("removed").and_then(Value::as_bool),
        Some(false)
    );
}

#[tokio::test]
async fn auth_cli_flows_cover_app_session_and_provider_storage_paths() {
    let _lock = env_lock();
    let harness = setup().await;

    let fields = parse_field_equals_entries(&[
        "scope=repo".to_string(),
        "refresh_token=refresh-1".to_string(),
    ])
    .expect("parse cli fields");
    assert_eq!(fields.get("scope").and_then(Value::as_str), Some("repo"));
    assert!(parse_field_equals_entries(&["not-key-value".to_string()]).is_err());
    assert!(parse_field_equals_entries(&[" =blank".to_string()]).is_err());

    let provider_login = cli_auth_login(
        " github ".to_string(),
        "provider-token".to_string(),
        None,
        None,
        fields,
        Some("work".to_string()),
        true,
    )
    .await
    .expect("provider cli login");
    assert!(
        provider_login.to_string().contains("github"),
        "provider login should mention provider: {provider_login}"
    );

    let provider_status = cli_auth_status("github".to_string(), None)
        .await
        .expect("provider status");
    assert!(
        provider_status.to_string().contains("github"),
        "provider status should include github profile: {provider_status}"
    );

    let provider_list = cli_auth_list(Some(" github ".to_string()))
        .await
        .expect("provider list");
    assert!(
        provider_list.to_string().contains("github"),
        "provider list should include github profile: {provider_list}"
    );

    let provider_logout = cli_auth_logout("github".to_string(), Some("work".to_string()))
        .await
        .expect("provider logout");
    assert!(
        provider_logout.to_string().contains("removed")
            || provider_logout.to_string().contains("true"),
        "provider logout should report removal: {provider_logout}"
    );

    let session_login = cli_auth_login(
        APP_SESSION_PROVIDER.to_string(),
        "header.payload.local".to_string(),
        Some("cli-user".to_string()),
        Some(json!({ "id": "cli-user", "name": "CLI User" })),
        Value::Object(Default::default()),
        None,
        true,
    )
    .await
    .expect("app-session cli login");
    assert!(
        session_login.to_string().contains("app-session")
            && session_login.to_string().contains("session stored"),
        "session login should store app session profile: {session_login}"
    );

    let session_status = cli_auth_status(APP_SESSION_PROVIDER.to_string(), None)
        .await
        .expect("session status");
    assert!(
        session_status.to_string().contains("isAuthenticated")
            || session_status.to_string().contains("cli-user"),
        "session status should expose auth state: {session_status}"
    );

    let session_logout = cli_auth_logout(APP_SESSION_PROVIDER.to_string(), None)
        .await
        .expect("session logout");
    assert!(
        session_logout.to_string().contains("isAuthenticated")
            || session_logout.to_string().contains("false")
            || session_logout.to_string().contains("removed"),
        "session logout should clear auth state: {session_logout}"
    );

    harness.join.abort();
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

    let unknown_app_state = app_state_schemas("missing");
    assert_eq!(unknown_app_state.namespace, "app_state");
    assert_eq!(unknown_app_state.function, "unknown");
    assert_eq!(unknown_app_state.outputs[0].name, "error");
    assert!(unknown_app_state.description.contains("Unknown app_state"));

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
async fn config_runtime_flags_settings_readbacks_and_validation_paths_are_exercised() {
    let _lock = env_lock();
    let harness = setup().await;

    let refused = rpc(
        &harness.rpc_base,
        11_001,
        "openhuman.config_set_browser_allow_all",
        json!({ "enabled": true }),
    )
    .await;
    assert_error_contains(
        &refused,
        "set_browser_allow_all true without operator opt-in",
        "Refusing to enable OPENHUMAN_BROWSER_ALLOW_ALL",
    );

    std::env::set_var("OPENHUMAN_BROWSER_ALLOW_ALL_RPC_ENABLE", "1");
    let enabled = rpc(
        &harness.rpc_base,
        11_002,
        "openhuman.config_set_browser_allow_all",
        json!({ "enabled": true }),
    )
    .await;
    assert_eq!(
        payload(&enabled, "set_browser_allow_all true")
            .get("browser_allow_all")
            .and_then(Value::as_bool),
        Some(true)
    );

    let disabled = rpc(
        &harness.rpc_base,
        11_003,
        "openhuman.config_set_browser_allow_all",
        json!({ "enabled": false }),
    )
    .await;
    assert_eq!(
        payload(&disabled, "set_browser_allow_all false")
            .get("browser_allow_all")
            .and_then(Value::as_bool),
        Some(false)
    );

    ok(
        &rpc(
            &harness.rpc_base,
            11_004,
            "openhuman.config_update_analytics_settings",
            json!({ "enabled": false }),
        )
        .await,
        "update_analytics_settings false",
    );
    let analytics = rpc(
        &harness.rpc_base,
        11_005,
        "openhuman.config_get_analytics_settings",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&analytics, "get_analytics_settings")
            .get("enabled")
            .and_then(Value::as_bool),
        Some(false)
    );

    ok(
        &rpc(
            &harness.rpc_base,
            11_006,
            "openhuman.config_update_meet_settings",
            json!({ "auto_orchestrator_handoff": true }),
        )
        .await,
        "update_meet_settings true",
    );
    let meet = rpc(
        &harness.rpc_base,
        11_007,
        "openhuman.config_get_meet_settings",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&meet, "get_meet_settings")
            .get("auto_orchestrator_handoff")
            .and_then(Value::as_bool),
        Some(true)
    );

    let onboarding_before = rpc(
        &harness.rpc_base,
        11_008,
        "openhuman.config_get_onboarding_completed",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&onboarding_before, "get_onboarding_completed before").as_bool(),
        Some(false)
    );
    for (id, value) in [(11_009, true), (11_010, false)] {
        let updated = rpc(
            &harness.rpc_base,
            id,
            "openhuman.config_set_onboarding_completed",
            json!({ "value": value }),
        )
        .await;
        assert_eq!(
            payload(&updated, "set_onboarding_completed").as_bool(),
            Some(value)
        );
    }

    ok(
        &rpc(
            &harness.rpc_base,
            11_011,
            "openhuman.config_update_dictation_settings",
            json!({
                "enabled": true,
                "hotkey": "Ctrl+Space",
                "activation_mode": "toggle",
                "llm_refinement": false,
                "streaming": true,
                "streaming_interval_ms": 750
            }),
        )
        .await,
        "update_dictation_settings valid",
    );
    let dictation = rpc(
        &harness.rpc_base,
        11_012,
        "openhuman.config_get_dictation_settings",
        json!({}),
    )
    .await;
    let dictation_payload = payload(&dictation, "get_dictation_settings");
    assert_eq!(
        dictation_payload
            .get("activation_mode")
            .and_then(Value::as_str),
        Some("toggle")
    );
    assert_eq!(
        dictation_payload
            .get("streaming_interval_ms")
            .and_then(Value::as_u64),
        Some(750)
    );

    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            11_013,
            "openhuman.config_update_dictation_settings",
            json!({ "activation_mode": "hold" }),
        )
        .await,
        "update_dictation_settings invalid activation",
        "invalid activation_mode",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            11_014,
            "openhuman.config_update_voice_server_settings",
            json!({ "activation_mode": "hold" }),
        )
        .await,
        "update_voice_server_settings invalid activation",
        "invalid activation_mode",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            11_015,
            "openhuman.config_update_search_settings",
            json!({ "engine": "bing" }),
        )
        .await,
        "update_search_settings invalid engine",
        "engine must be one of",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            11_016,
            "openhuman.config_update_search_settings",
            json!({ "max_results": 0 }),
        )
        .await,
        "update_search_settings invalid max_results",
        "max_results must be between",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            11_017,
            "openhuman.config_update_autonomy_settings",
            json!({ "level": "reckless" }),
        )
        .await,
        "update_autonomy_settings invalid level",
        "invalid autonomy level",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            11_018,
            "openhuman.config_update_model_settings",
            json!({
                "cloud_providers": [{
                    "slug": "",
                    "endpoint": "http://127.0.0.1:19999/v1",
                    "auth_style": "bearer"
                }]
            }),
        )
        .await,
        "update_model_settings empty cloud provider slug",
        "cloud provider slug must not be empty",
    );

    ok(
        &rpc(
            &harness.rpc_base,
            11_019,
            "openhuman.config_update_screen_intelligence_settings",
            json!({ "baseline_fps": 99.0 }),
        )
        .await,
        "update_screen_intelligence_settings clamps baseline",
    );
    ok(
        &rpc(
            &harness.rpc_base,
            11_020,
            "openhuman.config_update_voice_server_settings",
            json!({
                "min_duration_secs": -1.0,
                "silence_threshold": -0.5
            }),
        )
        .await,
        "update_voice_server_settings clamps non-negative floats",
    );
    let config = rpc(&harness.rpc_base, 11_021, "openhuman.config_get", json!({})).await;
    let config_payload = payload(&config, "config_get after clamps");
    assert_eq!(
        config_payload.pointer("/config/screen_intelligence/baseline_fps"),
        Some(&json!(30.0))
    );
    assert_eq!(
        config_payload.pointer("/config/voice_server/min_duration_secs"),
        Some(&json!(0.0))
    );
    assert_eq!(
        config_payload.pointer("/config/voice_server/silence_threshold"),
        Some(&json!(0.0))
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
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_019,
            "openhuman.auth_store_provider_credentials",
            json!({ "provider": "worker-a-empty" }),
        )
        .await,
        "auth_store_provider_credentials missing credential material",
        "provide at least one credential",
    );
    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            20_020,
            "openhuman.auth_store_session",
            json!({ "token": "header.payload.local" }),
        )
        .await,
        "auth_store_session local token without user",
        "local session requires a user payload",
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
async fn auth_local_session_normalizes_user_and_app_state_snapshot_uses_stored_identity() {
    let _lock = env_lock();
    let harness = setup().await;

    let session = rpc(
        &harness.rpc_base,
        21_001,
        "openhuman.auth_store_session",
        json!({
            "token": "header.payload.local",
            "user": {
                "id": "renderer-supplied-id",
                "name": "Local Worker",
                "email": "local-worker@example.test"
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&session, "auth_store_session local")
            .get("provider")
            .and_then(Value::as_str),
        Some("app-session")
    );

    let state = rpc(
        &harness.rpc_base,
        21_002,
        "openhuman.auth_get_state",
        json!({}),
    )
    .await;
    let state_payload = payload(&state, "auth_get_state after local session");
    assert_eq!(
        state_payload
            .get("isAuthenticated")
            .and_then(Value::as_bool),
        Some(true)
    );
    let user_id = state_payload
        .get("userId")
        .and_then(Value::as_str)
        .expect("local session should set userId");
    assert!(
        user_id.starts_with("local-"),
        "local session user id should be host-scoped, got {user_id:?}"
    );
    assert_eq!(
        state_payload.pointer("/user/id").and_then(Value::as_str),
        Some(user_id)
    );
    assert_eq!(
        state_payload.pointer("/user/_id").and_then(Value::as_str),
        Some(user_id)
    );

    let token = rpc(
        &harness.rpc_base,
        21_003,
        "openhuman.auth_get_session_token",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&token, "auth_get_session_token after local session")
            .get("token")
            .and_then(Value::as_str),
        Some("header.payload.local")
    );

    let snapshot = rpc(
        &harness.rpc_base,
        21_004,
        "openhuman.app_state_snapshot",
        json!({}),
    )
    .await;
    let snapshot_payload = payload(&snapshot, "app_state_snapshot local session");
    assert_eq!(
        snapshot_payload.get("sessionToken").and_then(Value::as_str),
        Some("header.payload.local")
    );
    assert_eq!(
        snapshot_payload
            .pointer("/currentUser/email")
            .and_then(Value::as_str),
        Some("local-worker@example.test")
    );

    let cleared = rpc(
        &harness.rpc_base,
        21_005,
        "openhuman.auth_clear_session",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&cleared, "auth_clear_session after local session")
            .get("removed")
            .and_then(Value::as_bool),
        Some(true)
    );

    harness.join.abort();
}

#[tokio::test]
async fn auth_remote_backend_paths_and_app_state_current_user_cache_round_trip() {
    let _lock = env_lock();
    let (backend_base, backend_state, backend_join) = serve_mock_backend().await;
    let harness = setup().await;
    let _backend_guard = EnvVarGuard::set("BACKEND_URL", &backend_base);

    let session = rpc(
        &harness.rpc_base,
        22_001,
        "openhuman.auth_store_session",
        json!({
            "token": "remote-jwt",
            "user_id": "remote-user-1",
            "user": {
                "id": "stale-renderer-user",
                "name": "Renderer Cache",
                "email": "renderer-cache@example.test"
            }
        }),
    )
    .await;
    assert_eq!(
        payload(&session, "auth_store_session remote")
            .get("provider")
            .and_then(Value::as_str),
        Some("app-session")
    );
    assert_eq!(
        backend_state.auth_me_hits.load(Ordering::SeqCst),
        1,
        "store_session should validate the JWT once"
    );

    let me = rpc(
        &harness.rpc_base,
        22_002,
        "openhuman.auth_get_me",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&me, "auth_get_me remote")
            .get("email")
            .and_then(Value::as_str),
        Some("remote-worker@example.test")
    );

    let consumed = rpc(
        &harness.rpc_base,
        22_003,
        "openhuman.auth_consume_login_token",
        json!({ "loginToken": "telegram-login-token" }),
    )
    .await;
    assert_eq!(
        payload(&consumed, "auth_consume_login_token remote")
            .get("jwtToken")
            .and_then(Value::as_str),
        Some("jwt-from-telegram-login-token")
    );

    let link = rpc(
        &harness.rpc_base,
        22_004,
        "openhuman.auth_create_channel_link_token",
        json!({ "channel": " Telegram " }),
    )
    .await;
    assert_eq!(
        payload(&link, "auth_create_channel_link_token remote")
            .get("linkToken")
            .and_then(Value::as_str),
        Some("link-token-123")
    );

    let integrations = rpc(
        &harness.rpc_base,
        22_005,
        "openhuman.auth_oauth_list_integrations",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&integrations, "auth_oauth_list_integrations remote")
            .pointer("/0/provider")
            .and_then(Value::as_str),
        Some("github")
    );

    let client_key = rpc(
        &harness.rpc_base,
        22_006,
        "openhuman.auth_oauth_fetch_client_key",
        json!({ "integrationId": "0123456789abcdef01234567" }),
    )
    .await;
    assert_eq!(
        payload(&client_key, "auth_oauth_fetch_client_key remote")
            .get("clientKey")
            .and_then(Value::as_str),
        Some("client-key-share")
    );

    let revoked = rpc(
        &harness.rpc_base,
        22_007,
        "openhuman.auth_oauth_revoke_integration",
        json!({ "integrationId": "0123456789abcdef01234567" }),
    )
    .await;
    assert_eq!(
        payload(&revoked, "auth_oauth_revoke_integration remote")
            .get("revoked")
            .and_then(Value::as_bool),
        Some(true)
    );

    assert_error_contains(
        &rpc(
            &harness.rpc_base,
            22_008,
            "openhuman.auth_oauth_fetch_integration_tokens",
            json!({ "integrationId": "short", "key": "secret" }),
        )
        .await,
        "auth_oauth_fetch_integration_tokens invalid id with session",
        "integrationId must be a 24-char hex id",
    );

    let snapshot = rpc(
        &harness.rpc_base,
        22_009,
        "openhuman.app_state_snapshot",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&snapshot, "app_state_snapshot remote")
            .pointer("/currentUser/email")
            .and_then(Value::as_str),
        Some("remote-worker@example.test")
    );
    let hits_after_first_snapshot = backend_state.auth_me_hits.load(Ordering::SeqCst);
    assert!(
        hits_after_first_snapshot >= 3,
        "store_session, auth_get_me, and snapshot should all touch /auth/me at least once"
    );

    let cached_snapshot = rpc(
        &harness.rpc_base,
        22_010,
        "openhuman.app_state_snapshot",
        json!({}),
    )
    .await;
    assert_eq!(
        payload(&cached_snapshot, "app_state_snapshot remote cached")
            .pointer("/currentUser/name")
            .and_then(Value::as_str),
        Some("Remote Worker")
    );
    assert_eq!(
        backend_state.auth_me_hits.load(Ordering::SeqCst),
        hits_after_first_snapshot,
        "the second snapshot should reuse the current-user cache"
    );

    let identity = openhuman_core::openhuman::app_state::peek_cached_current_user_identity()
        .expect("snapshot should seed cached identity");
    assert_eq!(identity.id.as_deref(), Some("remote-user-1"));
    assert_eq!(identity.name.as_deref(), Some("Remote Worker"));
    assert_eq!(
        identity.email.as_deref(),
        Some("remote-worker@example.test")
    );

    harness.join.abort();
    backend_join.abort();
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
async fn app_state_snapshot_quarantines_corrupted_local_state_file() {
    let _lock = env_lock();
    let harness = setup().await;

    let config = rpc(&harness.rpc_base, 31_001, "openhuman.config_get", json!({})).await;
    let workspace_dir = payload(&config, "config_get for app_state corruption")
        .get("workspace_dir")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .expect("config_get should expose workspace_dir");
    let state_dir = workspace_dir.join("state");
    std::fs::create_dir_all(&state_dir).expect("create state dir");
    let app_state_path = state_dir.join("app-state.json");
    std::fs::write(&app_state_path, "{ not valid json").expect("write corrupted app state");

    let snapshot = rpc(
        &harness.rpc_base,
        31_002,
        "openhuman.app_state_snapshot",
        json!({}),
    )
    .await;
    let local_state = payload(&snapshot, "app_state_snapshot after corrupt state")
        .get("localState")
        .expect("snapshot should include localState");
    assert!(
        local_state.as_object().is_some_and(|map| map.is_empty()),
        "corrupted app state should fall back to defaults: {local_state}"
    );
    assert!(
        !app_state_path.exists(),
        "corrupted app state file should be moved out of the live path"
    );
    let quarantined = std::fs::read_dir(&state_dir)
        .expect("read state dir")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("app-state.json.corrupted.")
        });
    assert!(
        quarantined,
        "corrupted app state file should be quarantined under {state_dir:?}"
    );

    harness.join.abort();
}

#[test]
fn credentials_profile_store_public_api_persists_updates_and_recovers_bad_files() {
    let _lock = env_lock();
    let _keyring_guard = EnvVarGuard::set("OPENHUMAN_KEYRING_BACKEND", "file");
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join("profiles");
    let store = AuthProfilesStore::new(&state_dir, true);

    let empty = store.load().expect("empty store load");
    assert!(empty.profiles.is_empty());

    let mut token_profile = AuthProfile::new_token("openai", "work", "sk-worker-a".to_string());
    token_profile
        .metadata
        .insert("region".to_string(), "test".to_string());
    let token_id = token_profile.id.clone();
    store
        .upsert_profile(token_profile, true)
        .expect("upsert token profile");

    let mut oauth_profile = AuthProfile::new_oauth(
        "github",
        "default",
        TokenSet {
            access_token: "gh-access".to_string(),
            refresh_token: Some("gh-refresh".to_string()),
            id_token: Some("gh-id".to_string()),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            token_type: Some("Bearer".to_string()),
            scope: Some("repo user".to_string()),
        },
    );
    oauth_profile.account_id = Some("acct-1".to_string());
    oauth_profile.workspace_id = Some("workspace-1".to_string());
    let oauth_id = oauth_profile.id.clone();
    store
        .upsert_profile(oauth_profile, false)
        .expect("upsert oauth profile");

    let loaded = store.load().expect("load stored profiles");
    assert_eq!(
        loaded.active_profiles.get("openai").map(String::as_str),
        Some(token_id.as_str())
    );
    assert_eq!(
        loaded
            .profiles
            .get(&token_id)
            .and_then(|profile| profile.token.as_deref()),
        Some("sk-worker-a")
    );
    assert_eq!(
        loaded
            .profiles
            .get(&oauth_id)
            .and_then(|profile| profile.token_set.as_ref())
            .map(|tokens| tokens.access_token.as_str()),
        Some("gh-access")
    );

    store
        .set_active_profile("github", &oauth_id)
        .expect("set active oauth profile");
    store
        .clear_active_profile("openai")
        .expect("clear active token profile");
    let updated = store
        .update_profile(&oauth_id, |profile| {
            profile
                .metadata
                .insert("updated".to_string(), "true".to_string());
            Ok(())
        })
        .expect("update oauth metadata");
    assert_eq!(
        updated.metadata.get("updated").map(String::as_str),
        Some("true")
    );

    assert!(
        store
            .remove_profile(&token_id)
            .expect("remove existing token profile"),
        "existing token profile should be removed"
    );
    assert!(
        !store
            .remove_profile(&token_id)
            .expect("remove missing token profile"),
        "missing token profile removal should be idempotent"
    );

    let after_remove = store.load().expect("load after remove");
    assert!(!after_remove.profiles.contains_key(&token_id));
    assert_eq!(
        after_remove
            .active_profiles
            .get("github")
            .map(String::as_str),
        Some(oauth_id.as_str())
    );

    let corrupt_dir = tmp.path().join("corrupt");
    std::fs::create_dir_all(&corrupt_dir).expect("create corrupt profile dir");
    let corrupt_path = corrupt_dir.join("auth-profiles.json");
    std::fs::write(&corrupt_path, "{ not valid json").expect("write corrupt profile store");
    let corrupt_store = AuthProfilesStore::new(&corrupt_dir, true);
    let recovered = corrupt_store
        .load()
        .expect("corrupt profile store recovers");
    assert!(recovered.profiles.is_empty());
    assert!(
        !corrupt_path.exists(),
        "corrupted auth profile store should be moved away"
    );
    assert!(
        std::fs::read_dir(&corrupt_dir)
            .expect("read corrupt dir")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-")),
        "corrupt auth profile store should leave a quarantine file"
    );

    let legacy_dir = tmp.path().join("legacy");
    std::fs::create_dir_all(&legacy_dir).expect("create legacy profile dir");
    std::fs::write(
        legacy_dir.join("auth-profiles.json"),
        json!({
            "schema_version": 0,
            "updated_at": "2026-01-01T00:00:00Z",
            "active_profiles": {},
            "profiles": {}
        })
        .to_string(),
    )
    .expect("write legacy profile store");
    let legacy = AuthProfilesStore::new(&legacy_dir, true)
        .load()
        .expect("legacy schema 0 should normalize");
    assert_eq!(legacy.schema_version, 1);

    let future_dir = tmp.path().join("future");
    std::fs::create_dir_all(&future_dir).expect("create future profile dir");
    std::fs::write(
        future_dir.join("auth-profiles.json"),
        json!({
            "schema_version": 99,
            "updated_at": "2026-01-01T00:00:00Z",
            "active_profiles": {},
            "profiles": {}
        })
        .to_string(),
    )
    .expect("write future profile store");
    let future_err = AuthProfilesStore::new(&future_dir, true)
        .load()
        .expect_err("future schema should fail");
    assert!(
        future_err
            .to_string()
            .contains("Unsupported auth profile schema version"),
        "unexpected future schema error: {future_err:#}"
    );
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
