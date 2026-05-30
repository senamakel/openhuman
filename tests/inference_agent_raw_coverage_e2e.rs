//! Focused raw/E2E coverage for inference and agent controller paths.
//!
//! The suite uses only temp workspaces and loopback HTTP mocks. It avoids live
//! model/provider calls while still exercising the public controller registry.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::{tempdir, TempDir};

use openhuman_core::core::all::RegisteredController;
use openhuman_core::openhuman::agent::harness::AgentDefinitionRegistry;
use openhuman_core::openhuman::agent::personality_paths::{
    filter_integrations, memory_subdir_for_suffix, memory_tree_subdir_for_suffix,
    resolve_personality_memory_md, resolve_personality_soul, session_raw_subdir_for_suffix,
    HasToolkit, PersonalityContext,
};
use openhuman_core::openhuman::agent::pformat::{
    build_registry, parse_call as parse_pformat_call, render_signature, render_signature_from_tool,
    PFormatParamType, PFormatRegistry, PFormatToolParams,
};
use openhuman_core::openhuman::agent::profiles::{
    AgentProfile, AgentProfileStore, AgentProfilesState, DEFAULT_PROFILE_ID,
};
use openhuman_core::openhuman::agent::prompts::{
    render_ambient_environment, render_subagent_system_prompt, render_tools, ConnectedIntegration,
    GatedIntegrationTool, LearnedContextData, NamespaceSummary, PromptContext, PromptTool,
    SubagentRenderOptions, SystemPromptBuilder, ToolCallFormat, UserIdentity,
};
use openhuman_core::openhuman::agent::task_board::{
    TaskApprovalMode, TaskBoard, TaskBoardCard, TaskBoardStore, TaskCardStatus,
};
use openhuman_core::openhuman::agent::task_dispatcher::build_task_prompt;
use openhuman_core::openhuman::agent::tool_policy::{
    AllowAllToolPolicy, GeneratedToolRuntimeContext, GeneratedToolRuntimePolicy,
    GeneratedToolRuntimePolicyConfig, GeneratedToolRuntimeRisk, RuntimeToolPolicyAction,
    ToolCallContext, ToolPolicy, ToolPolicyDecision, ToolPolicyRequest,
};
use openhuman_core::openhuman::agent::tools::PlanExitTool;
use openhuman_core::openhuman::agent::triage::envelope::{TriggerEnvelope, TriggerSource};
use openhuman_core::openhuman::agent::triage::routing::build_local_provider_with_config;
use openhuman_core::openhuman::agent::triage::{parse_triage_decision, ParseError, TriageAction};
use openhuman_core::openhuman::agent::{
    all_agent_controller_schemas, all_agent_registered_controllers,
};
use openhuman_core::openhuman::config::schema::cloud_providers::{
    AuthStyle as CloudAuthStyle, CloudProviderCreds,
};
use openhuman_core::openhuman::config::schema::LocalAiConfig;
use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::credentials::{AuthService, APP_SESSION_PROVIDER};
use openhuman_core::openhuman::inference::context_window_for_model;
use openhuman_core::openhuman::inference::presets::{
    all_presets, apply_preset_to_config, current_tier_from_config, device_supports_local_ai,
    mvp_presets, preset_for_tier, recommend_tier, should_default_to_cloud_fallback,
    supports_screen_summary, vision_mode_for_config, vision_mode_for_tier, ModelTier, VisionMode,
    MIN_RAM_GB_FOR_LOCAL_AI, MVP_MAX_TIER,
};
use openhuman_core::openhuman::inference::provider::factory::{
    auth_key_for_slug, create_chat_provider_from_string, provider_for_role,
    BYOK_INCOMPLETE_SENTINEL,
};
use openhuman_core::openhuman::inference::provider::temperature::{
    glob_match, temperature_for_model,
};
use openhuman_core::openhuman::inference::provider::UsageInfo;
use openhuman_core::openhuman::inference::provider::{
    format_anyhow_chain, is_budget_exhausted_message, is_openai_compatible_unknown_model_message,
    is_provider_config_rejection_message, sanitize_api_error, scrub_secret_patterns,
};
use openhuman_core::openhuman::inference::sentiment::local_ai_analyze_sentiment;
use openhuman_core::openhuman::inference::voice::hallucination::{
    is_hallucinated_output, HallucinationMode,
};
use openhuman_core::openhuman::inference::{
    all_inference_controller_schemas, all_inference_registered_controllers,
    all_local_ai_controller_schemas, all_local_ai_registered_controllers, DeviceProfile,
};
use openhuman_core::openhuman::todos::ops::BoardLocation;
use openhuman_core::openhuman::tools::Tool;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: tests in this file serialize env mutation with ENV_LOCK.
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }

    fn unset(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: tests in this file serialize env mutation with ENV_LOCK.
        unsafe { std::env::remove_var(key) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => {
                // SAFETY: the owning test keeps ENV_LOCK held until drop.
                unsafe { std::env::set_var(self.key, value) }
            }
            None => {
                // SAFETY: the owning test keeps ENV_LOCK held until drop.
                unsafe { std::env::remove_var(self.key) }
            }
        }
    }
}

struct IsolatedEnv {
    _home: TempDir,
    _workspace: TempDir,
    _home_guard: EnvVarGuard,
    _workspace_guard: EnvVarGuard,
    _config_guard: EnvVarGuard,
    _openhuman_dir_guard: EnvVarGuard,
}

#[derive(Clone)]
struct FakeIntegration {
    toolkit: String,
}

impl HasToolkit for FakeIntegration {
    fn toolkit_name(&self) -> &str {
        &self.toolkit
    }
}

fn isolated_env() -> IsolatedEnv {
    let home = tempdir().expect("home tempdir");
    let workspace = tempdir().expect("workspace tempdir");
    let home_guard = EnvVarGuard::set("HOME", home.path());
    let workspace_guard = EnvVarGuard::set("OPENHUMAN_WORKSPACE", workspace.path());
    let config_guard = EnvVarGuard::unset("OPENHUMAN_CONFIG_PATH");
    let openhuman_dir_guard = EnvVarGuard::unset("OPENHUMAN_DIR");
    IsolatedEnv {
        _home: home,
        _workspace: workspace,
        _home_guard: home_guard,
        _workspace_guard: workspace_guard,
        _config_guard: config_guard,
        _openhuman_dir_guard: openhuman_dir_guard,
    }
}

#[derive(Clone, Default)]
struct ProviderMockState {
    requests: Arc<Mutex<Vec<(String, Option<String>, Value)>>>,
}

async fn serve_provider_mock() -> (String, ProviderMockState) {
    let state = ProviderMockState::default();
    let app = Router::new()
        .route("/v1/models", get(provider_models))
        .route("/v1/chat/completions", post(provider_chat))
        .route("/missing/models", get(provider_missing_models))
        .route("/api/tags", get(ollama_tags))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind provider mock");
    let addr = listener.local_addr().expect("provider mock addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("provider mock serve");
    });
    (format!("http://{addr}"), state)
}

async fn provider_models(State(state): State<ProviderMockState>, headers: HeaderMap) -> Response {
    state.requests.lock().expect("requests").push((
        "models".to_string(),
        header(&headers, "authorization"),
        Value::Null,
    ));
    Json(json!({
        "object": "list",
        "data": [
            { "id": "demo-chat", "owned_by": "test-suite" },
            { "id": "demo-coder", "owned_by": "test-suite", "context_window": 8192 }
        ]
    }))
    .into_response()
}

async fn provider_missing_models() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "models unsupported" })),
    )
        .into_response()
}

async fn ollama_tags() -> Response {
    Json(json!({
        "models": [
            { "name": "gemma3:1b-it-qat", "model": "gemma3:1b-it-qat" },
            { "name": "bge-m3", "model": "bge-m3" }
        ]
    }))
    .into_response()
}

async fn provider_chat(
    State(state): State<ProviderMockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state.requests.lock().expect("requests").push((
        "chat".to_string(),
        header(&headers, "authorization"),
        body,
    ));
    Json(json!({
        "id": "chatcmpl-coverage",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "mocked provider reply" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 4, "completion_tokens": 5, "total_tokens": 9 }
    }))
    .into_response()
}

fn header(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn controller<'a>(
    controllers: &'a [RegisteredController],
    function: &str,
) -> &'a RegisteredController {
    controllers
        .iter()
        .find(|controller| controller.schema.function == function)
        .unwrap_or_else(|| panic!("controller {function} registered"))
}

async fn call(controller: &RegisteredController, params: Value) -> Result<Value, String> {
    let params = params.as_object().cloned().unwrap_or_default();
    (controller.handler)(params).await
}

#[tokio::test]
async fn inference_registry_drives_config_oauth_models_and_provider_chat() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _env = isolated_env();
    let (provider_base, provider_state) = serve_provider_mock().await;

    let schemas = all_inference_controller_schemas();
    let registered = all_inference_registered_controllers();
    assert_eq!(schemas.len(), registered.len());
    assert!(schemas
        .iter()
        .any(|schema| schema.function == "test_provider_model"));
    assert!(registered.iter().all(|controller| {
        controller
            .rpc_method_name()
            .starts_with("openhuman.inference_")
    }));

    let invalid_update = call(
        controller(&registered, "update_model_settings"),
        json!({
            "cloud_providers": [{
                "slug": "bad-auth",
                "endpoint": format!("{provider_base}/v1"),
                "auth_style": "digest"
            }]
        }),
    )
    .await
    .expect_err("invalid auth style should be rejected before saving");
    assert!(invalid_update.contains("unknown auth_style"));

    let updated = call(
        controller(&registered, "update_model_settings"),
        json!({
            "default_model": "agentic-v1",
            "default_temperature": 0.11,
            "primary_cloud": "mock",
            "chat_provider": "mock:demo-chat",
            "coding_provider": "mock:demo-coder@0.25",
            "cloud_providers": [
                {
                    "id": "mock-id",
                    "slug": "mock",
                    "label": "Mock Provider",
                    "endpoint": format!("{provider_base}/v1"),
                    "auth_style": "none",
                    "default_model": "demo-chat"
                },
                {
                    "slug": "openhuman",
                    "endpoint": "https://reserved.example/v1",
                    "auth_style": "none"
                }
            ],
            "model_routes": [{ "hint": "chat", "model": "mock:demo-chat" }]
        }),
    )
    .await
    .expect("valid model settings");
    assert_eq!(
        updated.pointer("/result/config/default_model"),
        Some(&json!("agentic-v1"))
    );
    assert_eq!(
        updated.pointer("/result/config/cloud_providers/0/slug"),
        Some(&json!("mock"))
    );

    let local = call(
        controller(&registered, "update_local_settings"),
        json!({
            "runtime_enabled": true,
            "opt_in_confirmed": true,
            "provider": "lmstudio",
            "base_url": format!("{provider_base}/v1"),
            "chat_model_id": "demo-chat",
            "usage_embeddings": false,
            "usage_heartbeat": true,
            "usage_learning_reflection": true,
            "usage_subconscious": false
        }),
    )
    .await
    .expect("valid local settings");
    assert_eq!(
        local.pointer("/result/config/local_ai/provider"),
        Some(&json!("lm_studio"))
    );

    let client_config = call(controller(&registered, "get_client_config"), json!({}))
        .await
        .expect("client config");
    assert_eq!(
        client_config.pointer("/result/default_model"),
        Some(&json!("agentic-v1"))
    );

    let config = Config::load_or_init().await.expect("load config");
    AuthService::from_config(&config)
        .store_provider_token(
            APP_SESSION_PROVIDER,
            "default",
            "session-token-for-custom-provider-gate",
            HashMap::new(),
            true,
        )
        .expect("store app session token");

    let models = call(
        controller(&registered, "list_models"),
        json!({ "provider_id": "mock-id" }),
    )
    .await
    .expect("models listed");
    assert_eq!(
        models.pointer("/result/models/0/id"),
        Some(&json!("demo-chat"))
    );

    let unknown = call(
        controller(&registered, "list_models"),
        json!({ "provider_id": "missing-provider" }),
    )
    .await
    .expect_err("unknown provider should be a user-config error");
    assert!(unknown.contains("no cloud provider with id or slug"));

    let reply = call(
        controller(&registered, "test_provider_model"),
        json!({
            "workload": "chat",
            "provider": "mock:demo-chat",
            "prompt": "hello from coverage"
        }),
    )
    .await
    .expect("provider chat succeeds through mock");
    assert_eq!(
        reply.pointer("/result/reply"),
        Some(&json!("mocked provider reply"))
    );

    let oauth_status = call(controller(&registered, "openai_oauth_status"), json!({}))
        .await
        .expect("oauth status");
    assert_eq!(
        oauth_status.pointer("/result/connected"),
        Some(&json!(false))
    );

    let oauth_start = call(controller(&registered, "openai_oauth_start"), json!({}))
        .await
        .expect("oauth start");
    let state = oauth_start
        .pointer("/result/state")
        .and_then(Value::as_str)
        .expect("state");
    assert!(!state.is_empty());
    assert_eq!(
        oauth_start.pointer("/result/redirectUri"),
        Some(&json!("http://127.0.0.1:1455/auth/callback"))
    );

    let mismatch = call(
        controller(&registered, "openai_oauth_complete"),
        json!({ "callbackUrl": "http://127.0.0.1:1455/auth/callback?code=abc&state=wrong" }),
    )
    .await
    .expect_err("state mismatch should stop before token exchange");
    assert!(mismatch.contains("OAuth state mismatch"));

    let disconnected = call(
        controller(&registered, "openai_oauth_disconnect"),
        json!({}),
    )
    .await
    .expect("disconnect is idempotent");
    assert_eq!(
        disconnected.pointer("/result/disconnected"),
        Some(&json!(false))
    );

    let invalid_complete = call(
        controller(&registered, "openai_oauth_complete"),
        json!({ "callback_url": "" }),
    )
    .await
    .expect_err("no pending session after mismatch");
    assert!(invalid_complete.contains("no pending OAuth session"));

    let requests = provider_state.requests.lock().expect("requests").clone();
    assert!(requests.iter().any(|(kind, _, _)| kind == "models"));
    let chat_request = requests
        .iter()
        .find(|(kind, _, _)| kind == "chat")
        .expect("chat request captured");
    assert_eq!(chat_request.2.pointer("/model"), Some(&json!("demo-chat")));
}

#[tokio::test]
async fn agent_registry_and_profile_controllers_cover_success_and_errors() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _env = isolated_env();
    AgentDefinitionRegistry::init_global_builtins().expect("init builtins");

    let schemas = all_agent_controller_schemas();
    let registered = all_agent_registered_controllers();
    assert_eq!(schemas.len(), registered.len());
    assert!(registered
        .iter()
        .all(|controller| controller.rpc_method_name().starts_with("openhuman.agent_")));

    let status = call(controller(&registered, "server_status"), json!({}))
        .await
        .expect("server status");
    assert_eq!(status.pointer("/result/running"), Some(&json!(true)));
    assert!(status.pointer("/result/url").is_some());

    let definitions = call(controller(&registered, "list_definitions"), json!({}))
        .await
        .expect("definitions");
    let defs = definitions
        .pointer("/definitions")
        .and_then(Value::as_array)
        .expect("definitions array");
    assert!(defs
        .iter()
        .any(|def| def.pointer("/id") == Some(&json!("planner"))));

    let planner = call(
        controller(&registered, "get_definition"),
        json!({ "id": " planner " }),
    )
    .await
    .expect("definition trims id");
    assert_eq!(planner.pointer("/definition/id"), Some(&json!("planner")));

    let missing_definition = call(
        controller(&registered, "get_definition"),
        json!({ "id": "definitely-not-real" }),
    )
    .await
    .expect_err("unknown definition");
    assert!(missing_definition.contains("definition 'definitely-not-real' not found"));

    let reload = call(controller(&registered, "reload_definitions"), json!({}))
        .await
        .expect("reload is noop");
    assert_eq!(reload.pointer("/status"), Some(&json!("noop")));
    assert_eq!(reload.pointer("/registry_initialised"), Some(&json!(true)));

    let list = call(controller(&registered, "profiles_list"), json!({}))
        .await
        .expect("profiles list");
    assert_eq!(
        list.pointer("/activeProfileId"),
        Some(&json!(DEFAULT_PROFILE_ID))
    );
    assert!(list
        .pointer("/profiles")
        .and_then(Value::as_array)
        .expect("profiles")
        .iter()
        .any(|profile| profile.pointer("/id") == Some(&json!("research"))));

    let unknown_agent = call(
        controller(&registered, "profile_upsert"),
        json!({
            "profile": {
                "id": "Bad Agent",
                "name": "Bad Agent",
                "description": "invalid agent id",
                "agentId": "unknown-agent-id"
            }
        }),
    )
    .await
    .expect_err("registry rejects unknown agent id");
    assert!(unknown_agent.contains("agent definition 'unknown-agent-id' not found"));

    let upserted = call(
        controller(&registered, "profile_upsert"),
        json!({
            "profile": {
                "id": " My Research Profile ",
                "name": "  My Research Profile  ",
                "description": "  focused work  ",
                "agentId": "planner",
                "modelOverride": " agentic-v1 ",
                "temperature": 0.4,
                "systemPromptSuffix": " be precise ",
                "allowedTools": [" memory_search ", "", " composio_execute_action "],
                "avatarUrl": " https://example.test/avatar.png ",
                "voiceId": " voice-a ",
                "soulMd": " custom soul ",
                "composioIntegrations": [" gmail ", "", "slack"]
            }
        }),
    )
    .await
    .expect("upsert profile");
    let custom = upserted
        .pointer("/profiles")
        .and_then(Value::as_array)
        .expect("profiles")
        .iter()
        .find(|profile| profile.pointer("/id") == Some(&json!("my-research-profile")))
        .expect("custom profile");
    assert_eq!(custom.pointer("/agentId"), Some(&json!("planner")));
    assert_eq!(custom.pointer("/memoryDirSuffix"), Some(&json!("-1")));
    assert_eq!(
        custom.pointer("/allowedTools"),
        Some(&json!(["memory_search", "composio_execute_action"]))
    );

    let selected = call(
        controller(&registered, "profile_select"),
        json!({ "profile_id": "my-research-profile" }),
    )
    .await
    .expect("select profile");
    assert_eq!(
        selected.pointer("/activeProfileId"),
        Some(&json!("my-research-profile"))
    );

    let missing_select = call(
        controller(&registered, "profile_select"),
        json!({ "profile_id": "missing-profile" }),
    )
    .await
    .expect_err("missing profile");
    assert!(missing_select.contains("agent profile 'missing-profile' not found"));

    let delete_builtin = call(
        controller(&registered, "profile_delete"),
        json!({ "profile_id": DEFAULT_PROFILE_ID }),
    )
    .await
    .expect_err("built-in profile cannot be deleted");
    assert!(delete_builtin.contains("built-in agent profile"));

    let deleted = call(
        controller(&registered, "profile_delete"),
        json!({ "profile_id": "my-research-profile" }),
    )
    .await
    .expect("delete custom profile");
    assert_eq!(
        deleted.pointer("/activeProfileId"),
        Some(&json!(DEFAULT_PROFILE_ID))
    );
}

#[test]
fn agent_profile_store_and_personality_helpers_cover_normalisation_edges() {
    let workspace = tempdir().expect("workspace");
    let store = AgentProfileStore::new(workspace.path().to_path_buf());

    let empty = store.load().expect("default profiles");
    assert_eq!(empty.active_profile_id, DEFAULT_PROFILE_ID);
    assert!(empty.profiles.iter().any(|profile| profile.id == "planner"));

    let first = store
        .upsert(AgentProfile {
            id: " Writing Buddy ".to_string(),
            name: " Writing Buddy ".to_string(),
            description: " drafts ".to_string(),
            agent_id: " planner ".to_string(),
            model_override: Some(" coding-v1 ".to_string()),
            temperature: Some(0.2),
            system_prompt_suffix: Some(" polish tone ".to_string()),
            allowed_tools: Some(vec![" memory_search ".to_string(), String::new()]),
            built_in: false,
            avatar_url: Some(" https://example.test/a.png ".to_string()),
            voice_id: Some(" voice-1 ".to_string()),
            soul_md: Some(" inline soul ".to_string()),
            soul_md_path: None,
            composio_integrations: Some(vec![" gmail ".to_string(), String::new()]),
            memory_dir_suffix: None,
            is_master: true,
            sort_order: Some(50),
        })
        .expect("upsert first");
    let writing = first
        .profiles
        .iter()
        .find(|profile| profile.id == "writing-buddy")
        .expect("writing profile");
    assert_eq!(writing.memory_dir_suffix.as_deref(), Some("-1"));
    assert!(!writing.is_master);

    let selected = store.select("writing-buddy").expect("select");
    assert_eq!(selected.active_profile_id, "writing-buddy");
    let (_, resolved) = store.resolve(None).expect("resolve active");
    assert_eq!(resolved.id, "writing-buddy");

    let second = store
        .upsert(AgentProfile {
            id: "Second".to_string(),
            name: "Second".to_string(),
            description: String::new(),
            agent_id: String::new(),
            model_override: None,
            temperature: None,
            system_prompt_suffix: None,
            allowed_tools: Some(vec![]),
            built_in: false,
            avatar_url: None,
            voice_id: None,
            soul_md: None,
            soul_md_path: None,
            composio_integrations: Some(vec![]),
            memory_dir_suffix: None,
            is_master: false,
            sort_order: None,
        })
        .expect("upsert second");
    let second_profile = second
        .profiles
        .iter()
        .find(|profile| profile.id == "second")
        .expect("second profile");
    assert_eq!(second_profile.agent_id, "orchestrator");
    assert_eq!(second_profile.allowed_tools, None);
    assert_eq!(second_profile.composio_integrations, None);
    assert_eq!(second_profile.memory_dir_suffix.as_deref(), Some("-2"));

    let reused = store
        .upsert(AgentProfile {
            memory_dir_suffix: None,
            description: "updated".to_string(),
            ..second_profile.clone()
        })
        .expect("reuse suffix");
    let second_profile = reused
        .profiles
        .iter()
        .find(|profile| profile.id == "second")
        .expect("second profile");
    assert_eq!(second_profile.memory_dir_suffix.as_deref(), Some("-2"));

    let deleted = store.delete("writing-buddy").expect("delete active custom");
    assert_eq!(deleted.active_profile_id, DEFAULT_PROFILE_ID);
    assert!(store.delete("missing").unwrap_err().contains("not found"));
    assert!(store.delete("review").unwrap_err().contains("built-in"));

    let bad_workspace = tempdir().expect("bad workspace");
    std::fs::write(
        bad_workspace.path().join("agent_profiles.json"),
        "{not json",
    )
    .expect("write bad profiles");
    let err = AgentProfileStore::new(bad_workspace.path().to_path_buf())
        .load()
        .expect_err("bad JSON");
    assert!(err.contains("parse agent profiles"));

    let mut suffixes = HashSet::new();
    for profile in store.load().expect("load final").profiles {
        if let Some(suffix) = profile.memory_dir_suffix {
            suffixes.insert(suffix);
        }
    }
    assert!(suffixes.contains(""));
}

#[test]
fn agent_profile_state_deserializes_legacy_shape_and_normalises_defaults() {
    let state: AgentProfilesState = serde_json::from_value(json!({
        "activeProfileId": "missing",
        "profiles": [
            {
                "id": "",
                "name": "   ",
                "description": "",
                "agentId": ""
            },
            {
                "id": "default",
                "name": "Custom Default",
                "description": "override default copy",
                "agentId": "planner",
                "memoryDirSuffix": "-should-be-ignored",
                "builtIn": false,
                "isMaster": false
            }
        ]
    }))
    .expect("legacy state");
    let workspace = tempdir().expect("workspace");
    let store = AgentProfileStore::new(workspace.path().to_path_buf());
    let saved = store.save(state).expect("save normalised");
    assert_eq!(saved.active_profile_id, DEFAULT_PROFILE_ID);
    let default_profile = saved
        .profiles
        .iter()
        .find(|profile| profile.id == DEFAULT_PROFILE_ID)
        .expect("default profile");
    assert_eq!(default_profile.agent_id, "planner");
    assert!(default_profile.is_master);
    assert_eq!(default_profile.memory_dir_suffix.as_deref(), Some(""));
    assert_eq!(default_profile.name, "Custom Default");
}

#[test]
fn agent_task_board_and_dispatcher_public_paths_cover_storage_and_prompt_shapes() {
    let workspace = tempdir().expect("workspace");
    let store = TaskBoardStore::new(workspace.path().to_path_buf());
    assert!(store.get("thread-1").expect("missing board").is_none());
    assert!(store
        .get("   ")
        .unwrap_err()
        .contains("invalid task board thread_id"));

    let mut board = TaskBoard::empty("thread-1");
    assert_eq!(board.thread_id, "thread-1");
    board.cards.push(TaskBoardCard {
        id: "card-1".into(),
        title: "Fallback title".into(),
        status: TaskCardStatus::Todo,
        objective: Some(" Ship the coverage branch ".into()),
        plan: vec!["Inspect gaps".into(), "Add tests".into()],
        assigned_agent: Some("planner".into()),
        allowed_tools: vec!["memory_recall".into()],
        approval_mode: Some(TaskApprovalMode::Required),
        acceptance_criteria: vec!["Focused tests pass".into()],
        evidence: vec![],
        notes: Some("Keep scope narrow".into()),
        blocker: None,
        source_metadata: Some(json!({
            "provider": "github",
            "repo": "tinyhumansai/openhuman",
            "external_id": "123",
            "url": "https://github.com/tinyhumansai/openhuman/issues/123"
        })),
        order: 2,
        updated_at: "2026-05-29T12:00:00Z".into(),
    });

    let saved = store.put(board).expect("put board");
    assert_eq!(saved.cards[0].status.as_str(), "todo");
    assert_eq!(
        saved.cards[0]
            .approval_mode
            .as_ref()
            .expect("approval mode")
            .as_str(),
        "required"
    );
    let loaded = store
        .get("thread-1")
        .expect("load board")
        .expect("board exists");
    assert_eq!(loaded.cards[0].id, "card-1");

    let prompt = build_task_prompt(&loaded.cards[0]);
    assert!(prompt.contains("Ship the coverage branch"));
    assert!(prompt.contains("1. Inspect gaps"));
    assert!(prompt.contains("Acceptance criteria"));
    assert!(prompt.contains("github tinyhumansai/openhuman#123"));
    assert!(prompt.contains("Source link: https://github.com"));
    assert!(prompt.contains("record the outcome on the upstream source"));

    let title_prompt = build_task_prompt(&TaskBoardCard {
        objective: Some("   ".into()),
        source_metadata: Some(json!({ "external_id": "123" })),
        ..loaded.cards[0].clone()
    });
    assert!(title_prompt.contains("Fallback title"));
    assert!(!title_prompt.contains("This task originates from #123"));

    let replaced = store
        .put(TaskBoard {
            thread_id: "thread-1".into(),
            cards: vec![],
            updated_at: String::new(),
        })
        .expect("replace board");
    assert!(replaced.cards.is_empty());
}

#[test]
fn agent_personality_paths_cover_safe_fallbacks_and_integration_filters() {
    let workspace = tempdir().expect("workspace");
    std::fs::create_dir_all(workspace.path().join("personalities/researcher"))
        .expect("create personality dir");
    std::fs::write(
        workspace.path().join("personalities/researcher/MEMORY.md"),
        "research memory",
    )
    .expect("write memory");
    std::fs::write(workspace.path().join("SOUL.md"), "root soul").expect("write root soul");
    std::fs::write(workspace.path().join("personality-soul.md"), "file soul")
        .expect("write personality soul");

    assert_eq!(memory_subdir_for_suffix(""), "memory");
    assert_eq!(memory_subdir_for_suffix("-2"), "memory-2");
    assert_eq!(memory_tree_subdir_for_suffix(""), "memory_tree");
    assert_eq!(memory_tree_subdir_for_suffix("-3"), "memory_tree-3");
    assert_eq!(session_raw_subdir_for_suffix(""), "session_raw");
    assert_eq!(session_raw_subdir_for_suffix("-4"), "session_raw-4");

    let mut profile = AgentProfile {
        id: "researcher".into(),
        name: "Researcher".into(),
        description: "Research".into(),
        agent_id: "planner".into(),
        model_override: None,
        temperature: None,
        system_prompt_suffix: None,
        allowed_tools: None,
        built_in: false,
        avatar_url: None,
        voice_id: Some("voice-research".into()),
        soul_md: Some("inline soul".into()),
        soul_md_path: Some("personality-soul.md".into()),
        composio_integrations: Some(vec!["gmail".into(), "slack".into()]),
        memory_dir_suffix: Some("-7".into()),
        is_master: false,
        sort_order: Some(10),
    };

    assert_eq!(
        resolve_personality_soul(workspace.path(), &profile).as_deref(),
        Some("file soul")
    );
    profile.soul_md_path = Some("../escape.md".into());
    assert_eq!(
        resolve_personality_soul(workspace.path(), &profile).as_deref(),
        Some("inline soul")
    );
    profile.soul_md_path = Some("missing.md".into());
    assert_eq!(
        resolve_personality_soul(workspace.path(), &profile).as_deref(),
        Some("inline soul")
    );
    assert_eq!(
        resolve_personality_memory_md(workspace.path(), &profile).as_deref(),
        Some("research memory")
    );

    let context = PersonalityContext::from_profile(workspace.path(), profile);
    assert_eq!(context.memory_suffix, "-7");
    assert_eq!(context.voice_id.as_deref(), Some("voice-research"));
    assert_eq!(
        context.composio_allowlist.as_deref(),
        Some(&["gmail".to_string(), "slack".to_string()][..])
    );

    let integrations = vec![
        FakeIntegration {
            toolkit: "gmail".into(),
        },
        FakeIntegration {
            toolkit: "notion".into(),
        },
        FakeIntegration {
            toolkit: "SLACK".into(),
        },
    ];
    assert_eq!(filter_integrations(&integrations, None).len(), 3);
    assert_eq!(filter_integrations(&integrations, Some(&[])).len(), 0);
    let allowed = vec!["slack".to_string(), "gmail".to_string()];
    let filtered = filter_integrations(&integrations, Some(&allowed));
    assert_eq!(filtered.len(), 2);
    assert!(filtered.iter().any(|item| item.toolkit == "gmail"));
    assert!(filtered.iter().any(|item| item.toolkit == "SLACK"));
}

#[tokio::test]
async fn inference_public_helpers_cover_context_windows_and_sentiment_fallbacks() {
    assert_eq!(context_window_for_model("gpt-4.1-mini"), Some(1_047_576));
    assert_eq!(
        context_window_for_model("claude-3-5-haiku-latest"),
        Some(200_000)
    );
    assert_eq!(context_window_for_model("o3-mini"), Some(200_000));
    assert_eq!(context_window_for_model("unknown-model"), None);
    assert_eq!(context_window_for_model("   "), None);

    let empty = local_ai_analyze_sentiment(&Config::default(), "   ")
        .await
        .expect("empty sentiment falls back to neutral");
    assert_eq!(empty.value.emotion, "neutral");
    assert_eq!(empty.value.valence, "neutral");
    assert_eq!(empty.value.confidence, 1.0);
}

#[tokio::test]
async fn inference_provider_factory_and_classifiers_cover_user_state_edges() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _env = isolated_env();
    let mut config = Config::load_or_init().await.expect("load config");

    AuthService::from_config(&config)
        .store_provider_token(
            APP_SESSION_PROVIDER,
            "default",
            "session-token-for-provider-factory",
            HashMap::new(),
            true,
        )
        .expect("store app session token");

    assert_eq!(auth_key_for_slug("openrouter"), "provider:openrouter");
    assert!(is_budget_exhausted_message(
        "OpenRouter says insufficient balance, add credits"
    ));
    assert!(!is_budget_exhausted_message("upstream timeout"));

    for body in [
        "The supported API model names are native-a or native-b",
        "ModelNotAllowed",
        "invalid_authentication_error",
        "unknown parameter: tools",
        "requires a subscription, upgrade for access",
        "No active credentials for provider: openai",
    ] {
        assert!(
            is_provider_config_rejection_message(body),
            "{body:?} should be user configuration state"
        );
    }
    assert!(is_openai_compatible_unknown_model_message(
        "Model `gpt-unknown` is not available. Use GET /openai/v1/models to list available models."
    ));
    assert!(!is_provider_config_rejection_message(
        "internal server error while streaming tokens"
    ));

    let scrubbed =
        scrub_secret_patterns("tokens sk-live-secret and github_pat_abc123 should not escape");
    assert!(scrubbed.contains("[REDACTED]"));
    assert!(!scrubbed.contains("sk-live-secret"));
    assert!(!sanitize_api_error(&"x".repeat(500)).contains(&"x".repeat(250)));
    let chain = format_anyhow_chain(&anyhow::anyhow!(
        "wrapped failure caused by ghp_secretvalue"
    ));
    assert!(chain.contains("[REDACTED]"));

    assert!(glob_match("moonshot*k2*", "moonshot/kimi-k2-instruct"));
    assert!(!glob_match("gpt*mini", "gpt-4o-large"));
    config.temperature_unsupported_models = vec!["gpt-5*".into(), "*kimi-k2*".into()];
    assert_eq!(temperature_for_model("gpt-5.5", 0.7, &config), None);
    assert_eq!(
        temperature_for_model("moonshot/kimi-k2-instruct", 0.7, &config),
        None
    );
    assert_eq!(
        temperature_for_model("gpt-4o-mini", 0.3, &config),
        Some(0.3)
    );

    config.default_model = Some("stale-provider-model".into());
    let (_, openhuman_model) =
        create_chat_provider_from_string("chat", "openhuman", &config).expect("openhuman provider");
    assert_eq!(openhuman_model, "reasoning-v1");

    let byok_err = provider_factory_error("chat", BYOK_INCOMPLETE_SENTINEL, &config);
    assert!(byok_err.contains("BYOK_INCOMPLETE"));

    let empty_ollama = provider_factory_error("chat", "ollama:", &config);
    assert!(empty_ollama.contains("empty model"));
    let empty_slug = provider_factory_error("chat", ":demo", &config);
    assert!(empty_slug.contains("empty slug"));
    let unknown = provider_factory_error("chat", "not-a-provider", &config);
    assert!(unknown.contains("unrecognised provider string"));

    config.cloud_providers = vec![CloudProviderCreds {
        id: "mock-id".into(),
        slug: "mock".into(),
        label: "Mock".into(),
        endpoint: "http://127.0.0.1:1/v1".into(),
        auth_style: CloudAuthStyle::None,
        legacy_type: None,
        default_model: Some("mock-default".into()),
    }];
    config.chat_provider = Some("mock:chat-model@0.25".into());
    config.reasoning_provider = None;
    config.memory_provider = None;
    assert_eq!(provider_for_role("chat", &config), "mock:chat-model@0.25");
    assert_eq!(
        provider_for_role("reasoning", &config),
        "mock:chat-model@0.25"
    );
    assert_eq!(provider_for_role("memory", &config), "openhuman");
}

fn provider_factory_error(role: &str, provider: &str, config: &Config) -> String {
    match create_chat_provider_from_string(role, provider, config) {
        Ok((_, model)) => panic!("provider factory unexpectedly succeeded with model {model}"),
        Err(err) => err.to_string(),
    }
}

#[test]
fn inference_voice_and_triage_parsers_cover_public_error_shapes() {
    assert!(is_hallucinated_output(
        "[ blank_audio ]",
        HallucinationMode::Conversation
    ));
    assert!(is_hallucinated_output(
        "Thank you. Thank you. Thank you.",
        HallucinationMode::Conversation
    ));
    assert!(is_hallucinated_output(
        "it it it it it it hello",
        HallucinationMode::Conversation
    ));
    assert!(is_hallucinated_output("okay", HallucinationMode::Dictation));
    assert!(!is_hallucinated_output(
        "okay",
        HallucinationMode::Conversation
    ));
    assert!(!is_hallucinated_output(
        "no no no please stop",
        HallucinationMode::Conversation
    ));

    let fenced = parse_triage_decision(
        "notes before\n```json\n{\"action\":\"ESCALATE\",\"target_agent\":\"orchestrator\",\"prompt\":\"draft a reply\",\"reason\":\"requires planning\",}\n```\ntrailing notes",
    )
    .expect("fenced triage");
    assert_eq!(fenced.action, TriageAction::Escalate);
    assert_eq!(fenced.target_agent.as_deref(), Some("orchestrator"));
    assert_eq!(fenced.prompt.as_deref(), Some("draft a reply"));

    let last_object = parse_triage_decision(
        "{\"action\":\"react\",\"target_agent\":\"trigger_reactor\",\"prompt\":\"first\",\"reason\":\"old\"} then {\"action\":\"drop\",\"reason\":\"duplicate\"}",
    )
    .expect("last object wins");
    assert_eq!(last_object.action.as_str(), "drop");
    assert_eq!(last_object.reason, "duplicate");

    let missing_target =
        parse_triage_decision("{\"action\":\"react\",\"reason\":\"needs side effect\"}")
            .expect_err("react must include target and prompt");
    assert!(matches!(
        missing_target,
        ParseError::MissingTarget { action: "react" }
    ));
    assert!(matches!(
        parse_triage_decision("no json here").expect_err("json required"),
        ParseError::NoJsonObject
    ));
}

#[tokio::test]
async fn agent_runtime_policy_cost_and_triage_helpers_cover_public_edges() {
    let request = ToolPolicyRequest::new(
        "email.send",
        json!({ "to": "user@example.test", "body": "secret body" }),
        ToolCallContext::session(
            "session-secret-123",
            "private-channel",
            "orchestrator",
            "call-1",
            7,
        ),
    );
    let debug = format!("{request:?}");
    assert!(debug.contains("sess..."));
    assert!(debug.contains("priv..."));
    assert!(!debug.contains("session-secret-123"));
    assert!(!debug.contains("secret body"));

    let allow_all = AllowAllToolPolicy;
    assert_eq!(allow_all.name(), "allow_all");
    assert_eq!(allow_all.check(&request).await, ToolPolicyDecision::Allow);
    assert_eq!(
        request.context.source,
        openhuman_core::openhuman::agent::tool_policy::ToolCallSource::Session
    );

    let generated = request
        .clone()
        .with_generated_tool_context(GeneratedToolRuntimeContext {
            provider_id: "mail.runtime".to_string(),
            capability_id: "email.send".to_string(),
            risk: GeneratedToolRuntimeRisk::ExternalWrite,
            source_digest: Some("sha256:abc".to_string()),
            approval_id: Some("approval-1".to_string()),
        });

    let disabled = GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig::default());
    assert_eq!(disabled.name(), "generated_tool_runtime");
    assert_eq!(disabled.check(&generated).await, ToolPolicyDecision::Allow);

    let missing_context = GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig {
        enabled: true,
        ..Default::default()
    });
    assert_eq!(
        missing_context.check(&request).await,
        ToolPolicyDecision::Allow
    );

    let revoked_provider = GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig {
        enabled: true,
        revoked_providers: BTreeSet::from(["mail.runtime".to_string()]),
        ..Default::default()
    });
    let denied = revoked_provider.check(&generated).await;
    assert!(matches!(denied, ToolPolicyDecision::Deny { .. }));
    assert!(denied
        .blocking_reason()
        .expect("deny reason")
        .contains("provider `mail.runtime` is revoked"));

    let revoked_capability = GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig {
        enabled: true,
        revoked_capabilities: BTreeSet::from(["email.send".to_string()]),
        ..Default::default()
    });
    let denied = revoked_capability.check(&generated).await;
    assert!(matches!(denied, ToolPolicyDecision::Deny { .. }));
    assert!(denied
        .blocking_reason()
        .expect("deny reason")
        .contains("capability `email.send` is revoked"));

    let capability_over_provider =
        GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig {
            enabled: true,
            provider_actions: BTreeMap::from([(
                "mail.runtime".to_string(),
                RuntimeToolPolicyAction::Allow,
            )]),
            capability_actions: BTreeMap::from([(
                "email.send".to_string(),
                RuntimeToolPolicyAction::RequireApproval,
            )]),
            ..Default::default()
        });
    let approval = capability_over_provider.check(&generated).await;
    assert!(matches!(
        approval,
        ToolPolicyDecision::RequireApproval { .. }
    ));
    assert!(approval
        .blocking_reason()
        .expect("approval reason")
        .contains("capability `email.send` matched runtime policy"));

    let provider_denial = GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig {
        enabled: true,
        provider_actions: BTreeMap::from([(
            "mail.runtime".to_string(),
            RuntimeToolPolicyAction::Deny,
        )]),
        ..Default::default()
    });
    assert!(matches!(
        provider_denial.check(&generated).await,
        ToolPolicyDecision::Deny { .. }
    ));

    let risk_approval = GeneratedToolRuntimePolicy::new(GeneratedToolRuntimePolicyConfig {
        enabled: true,
        risk_actions: BTreeMap::from([(
            GeneratedToolRuntimeRisk::ExternalWrite,
            RuntimeToolPolicyAction::RequireApproval,
        )]),
        ..Default::default()
    });
    assert!(matches!(
        risk_approval.check(&generated).await,
        ToolPolicyDecision::RequireApproval { .. }
    ));

    assert_eq!(
        GeneratedToolRuntimeRisk::Read < GeneratedToolRuntimeRisk::Write,
        true
    );
    assert_eq!(
        GeneratedToolRuntimeRisk::Execute < GeneratedToolRuntimeRisk::Dangerous,
        true
    );
    assert_eq!(ToolPolicyDecision::Allow.blocking_reason(), None);

    let usage = UsageInfo {
        input_tokens: 2_000_000,
        output_tokens: 1_000_000,
        cached_input_tokens: 1_000_000,
        charged_amount_usd: 0.0,
        ..Default::default()
    };
    assert_eq!(
        openhuman_core::openhuman::agent::cost::lookup_pricing("claude-opus-4.7").model,
        "reasoning-v1"
    );
    assert_eq!(
        openhuman_core::openhuman::agent::cost::lookup_pricing("unknown-model").model,
        "<fallback>"
    );
    let estimated =
        openhuman_core::openhuman::agent::cost::estimate_call_cost_usd("agentic-v1", &usage);
    assert!((estimated - 18.3).abs() < 1e-6, "got {estimated}");
    let charged = UsageInfo {
        charged_amount_usd: 0.42,
        ..usage.clone()
    };
    assert_eq!(
        openhuman_core::openhuman::agent::cost::call_cost_usd("reasoning-v1", &charged),
        0.42
    );
    let mut turn_cost = openhuman_core::openhuman::agent::cost::TurnCost::new();
    turn_cost.add_call("agentic-v1", &usage);
    turn_cost.add_call("reasoning-v1", &charged);
    assert_eq!(turn_cost.input_tokens, 4_000_000);
    assert_eq!(turn_cost.output_tokens, 2_000_000);
    assert_eq!(turn_cost.cached_input_tokens, 2_000_000);
    assert_eq!(turn_cost.charged_usd, 0.42);
    assert_eq!(turn_cost.call_count, 2);
    assert!(turn_cost.total_usd() > 18.7);

    let composio = TriggerEnvelope::from_composio(
        "gmail",
        "GMAIL_NEW_MESSAGE",
        "metadata-id",
        "metadata-uuid",
        json!({ "subject": "coverage" }),
    );
    assert_eq!(composio.source.slug(), "composio");
    assert_eq!(composio.external_id, "metadata-uuid");
    assert_eq!(composio.display_label, "composio/gmail/GMAIL_NEW_MESSAGE");
    assert!(matches!(composio.source, TriggerSource::Composio { .. }));

    let fallback_id =
        TriggerEnvelope::from_composio("notion", "PAGE_UPDATED", "metadata-id", "", json!({}));
    assert_eq!(fallback_id.external_id, "metadata-id");

    let webhook =
        TriggerEnvelope::from_webhook("tunnel-1", "POST", "/hooks/coverage", json!({ "ok": true }));
    assert_eq!(webhook.source.slug(), "webhook");
    assert_eq!(webhook.external_id, "tunnel-1");
    assert_eq!(webhook.display_label, "webhook/POST//hooks/coverage");

    let cron = TriggerEnvelope::from_cron("job-1", "daily-summary", "done");
    assert_eq!(cron.source.slug(), "cron");
    assert_eq!(cron.payload.pointer("/output"), Some(&json!("done")));

    let external = TriggerEnvelope::from_external("caller-1", "manual", json!({ "x": 1 }))
        .with_task_card(
            "card-1".to_string(),
            BoardLocation::Thread {
                workspace_dir: tempdir().expect("thread workspace").path().to_path_buf(),
                thread_id: "thread-1".to_string(),
            },
        );
    assert_eq!(external.source.slug(), "external");
    let link = external.card_link.expect("task card link");
    assert_eq!(link.card_id, "card-1");
    assert_eq!(link.location.thread_id(), Some("thread-1"));

    let webview = TriggerSource::WebviewIntegration {
        provider: "gmail".to_string(),
        account_id: "acct-1".to_string(),
    };
    assert_eq!(webview.slug(), "webview");

    let mut config = Config::default();
    assert!(build_local_provider_with_config(&config).is_none());
    config.local_ai.runtime_enabled = true;
    config.local_ai.chat_model_id = String::new();
    assert!(build_local_provider_with_config(&config).is_none());
    config.local_ai.provider = "custom_openai".to_string();
    config.local_ai.base_url = Some("http://127.0.0.1:9999/v1".to_string());
    config.local_ai.api_key = Some("local-key".to_string());
    config.local_ai.chat_model_id = "local-chat".to_string();
    let local = build_local_provider_with_config(&config).expect("local provider");
    assert_eq!(local.provider_name, "custom_openai");
    assert_eq!(local.model, "local-chat");
    assert!(local.used_local);
}

#[tokio::test]
async fn inference_local_controllers_and_presets_cover_public_paths() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _env = isolated_env();
    let (provider_base, _provider_state) = serve_provider_mock().await;

    let local_schemas = all_local_ai_controller_schemas();
    let local_registered = all_local_ai_registered_controllers();
    assert_eq!(local_schemas.len(), local_registered.len());
    assert!(local_registered.iter().all(|controller| controller
        .rpc_method_name()
        .starts_with("openhuman.local_ai_")));

    let reachable = call(
        controller(&local_registered, "test_connection"),
        json!({ "url": provider_base }),
    )
    .await
    .expect("mock ollama tags endpoint is reachable");
    assert_eq!(reachable.pointer("/reachable"), Some(&json!(true)));
    assert_eq!(reachable.pointer("/models_count"), Some(&json!(2)));

    let rejected_url = call(
        controller(&local_registered, "test_connection"),
        json!({ "url": "ftp://example.test" }),
    )
    .await
    .expect_err("non-http URL should be rejected");
    assert!(rejected_url.contains("URL must start with http:// or https://"));

    let assets = call(controller(&local_registered, "assets_status"), json!({}))
        .await
        .expect("local assets status");
    assert!(assets.is_object());

    let downloads = call(
        controller(&local_registered, "downloads_progress"),
        json!({}),
    )
    .await
    .expect("download progress");
    assert!(downloads.is_object());

    let whisper_status = call(
        controller(&local_registered, "whisper_install_status"),
        json!({}),
    )
    .await
    .expect("whisper install status");
    assert_eq!(whisper_status.pointer("/engine"), Some(&json!("whisper")));

    let piper_status = call(
        controller(&local_registered, "piper_install_status"),
        json!({}),
    )
    .await
    .expect("piper install status");
    assert_eq!(piper_status.pointer("/engine"), Some(&json!("piper")));

    let inference_registered = all_inference_registered_controllers();
    let status = call(controller(&inference_registered, "status"), json!({}))
        .await
        .expect("inference status");
    assert!(status.pointer("/result/state").is_some());

    let device = call(
        controller(&inference_registered, "device_profile"),
        json!({}),
    )
    .await
    .expect("device profile");
    assert!(device.pointer("/result/total_ram_bytes").is_some());

    let diagnostics = call(controller(&inference_registered, "diagnostics"), json!({}))
        .await
        .expect("diagnostics");
    assert!(diagnostics.pointer("/ok").is_some());

    let disabled = call(
        controller(&inference_registered, "apply_preset"),
        json!({ "tier": "disabled" }),
    )
    .await
    .expect("disable local ai preset");
    assert_eq!(
        disabled.pointer("/result/local_ai_enabled"),
        Some(&json!(false))
    );

    let bad_tier = call(
        controller(&inference_registered, "apply_preset"),
        json!({ "tier": "ram_16_plus_gb" }),
    )
    .await
    .expect_err("MVP build rejects larger preset tiers");
    assert!(bad_tier.contains("not available in this build"));

    let applied = call(
        controller(&inference_registered, "apply_preset"),
        json!({ "tier": "low" }),
    )
    .await
    .expect("low alias applies MVP preset");
    assert_eq!(
        applied.pointer("/result/applied_tier"),
        Some(&json!("ram_2_4gb"))
    );
    assert_eq!(
        applied.pointer("/result/vision_mode"),
        Some(&json!("disabled"))
    );

    let presets = call(controller(&inference_registered, "presets"), json!({}))
        .await
        .expect("presets controller");
    assert_eq!(
        presets.pointer("/result/recommended_tier"),
        Some(&json!("ram_2_4gb"))
    );
    assert_eq!(
        presets.pointer("/result/selected_tier"),
        Some(&json!("ram_2_4gb"))
    );

    assert_eq!(MVP_MAX_TIER, ModelTier::Ram2To4Gb);
    assert_eq!(MIN_RAM_GB_FOR_LOCAL_AI, 8);
    assert_eq!(all_presets().len(), 5);
    assert_eq!(mvp_presets().len(), 1);
    assert_eq!(
        ModelTier::from_str_opt("HIGH"),
        Some(ModelTier::Ram16PlusGb)
    );
    assert_eq!(ModelTier::from_str_opt("tier_1gb"), Some(ModelTier::Ram1Gb));
    assert_eq!(ModelTier::from_str_opt("bogus"), None);
    assert_eq!(
        preset_for_tier(ModelTier::Ram4To8Gb)
            .expect("4-8 preset")
            .vision_mode,
        VisionMode::Ondemand
    );
    assert!(preset_for_tier(ModelTier::Custom).is_none());
    assert_eq!(vision_mode_for_tier(ModelTier::Custom), VisionMode::Bundled);

    let tiny_device = test_device(4);
    let capable_device = test_device(16);
    assert!(!device_supports_local_ai(&tiny_device));
    assert!(should_default_to_cloud_fallback(&tiny_device));
    assert!(device_supports_local_ai(&capable_device));
    assert!(!should_default_to_cloud_fallback(&capable_device));
    assert_eq!(recommend_tier(&capable_device), ModelTier::Ram2To4Gb);

    let mut config = LocalAiConfig::default();
    apply_preset_to_config(&mut config, ModelTier::Ram4To8Gb);
    assert_eq!(current_tier_from_config(&config), ModelTier::Ram4To8Gb);
    assert_eq!(vision_mode_for_config(&config), VisionMode::Ondemand);
    assert!(supports_screen_summary(&config));

    config.selected_tier = Some("custom".into());
    assert_eq!(current_tier_from_config(&config), ModelTier::Custom);
    config.vision_model_id.clear();
    assert_eq!(vision_mode_for_config(&config), VisionMode::Disabled);
    config.vision_model_id = "custom-vision".into();
    config.preload_vision_model = false;
    assert_eq!(vision_mode_for_config(&config), VisionMode::Ondemand);
    config.preload_vision_model = true;
    assert_eq!(vision_mode_for_config(&config), VisionMode::Bundled);
}

#[test]
fn agent_pformat_and_prompt_renderers_cover_public_paths() {
    let plan_tool: Box<dyn Tool> = Box::new(PlanExitTool::new());
    let tools: Vec<Box<dyn Tool>> = vec![plan_tool];
    let registry = build_registry(&tools);
    assert_eq!(
        render_signature_from_tool(tools[0].as_ref()),
        "plan_exit[plan]"
    );
    assert_eq!(
        render_signature("plan_exit", registry.get("plan_exit").expect("plan params")),
        "plan_exit[plan]"
    );
    let (name, args) = parse_pformat_call(r"plan_exit[Read code \| add test \] commit]", &registry)
        .expect("p-format call parses");
    assert_eq!(name, "plan_exit");
    assert_eq!(
        args.pointer("/plan"),
        Some(&json!("Read code | add test ] commit"))
    );
    assert!(parse_pformat_call("bad-name[value]", &registry).is_none());

    let mut custom_registry = PFormatRegistry::new();
    custom_registry.insert(
        "coerce".into(),
        PFormatToolParams {
            names: vec![
                "flag".into(),
                "count".into(),
                "ratio".into(),
                "blob".into(),
                "maybe".into(),
            ],
            types: vec![
                PFormatParamType::Boolean,
                PFormatParamType::Integer,
                PFormatParamType::Number,
                PFormatParamType::Other,
                PFormatParamType::String,
            ],
        },
    );
    let (_, coerced) = parse_pformat_call("coerce[yes|7|2.5|{\"x\":1}|plain]", &custom_registry)
        .expect("custom p-format");
    assert_eq!(
        coerced,
        json!({
            "flag": true,
            "count": 7,
            "ratio": 2.5,
            "blob": "{\"x\":1}",
            "maybe": "plain"
        })
    );
    assert_eq!(
        PFormatParamType::from_schema_type(Some(&json!(["null", "integer"]))),
        PFormatParamType::Integer
    );
    assert_eq!(
        PFormatToolParams::from_schema(&json!({ "type": "string" })).names,
        Vec::<String>::new()
    );

    let workspace = tempdir().expect("prompt workspace");
    std::fs::write(workspace.path().join("SOUL.md"), "coverage soul").expect("write soul");
    std::fs::write(workspace.path().join("IDENTITY.md"), "coverage identity")
        .expect("write identity");
    std::fs::write(workspace.path().join("PROFILE.md"), "coverage profile").expect("write profile");
    std::fs::write(workspace.path().join("MEMORY.md"), "coverage memory").expect("write memory");

    let visible_tool_names = HashSet::from(["plan_exit".to_string()]);
    let prompt_tools = PromptTool::from_tools(&tools);
    let skills = Vec::new();
    let integrations = vec![ConnectedIntegration {
        toolkit: "gmail".into(),
        description: "Email account".into(),
        tools: vec![],
        gated_tools: vec![GatedIntegrationTool {
            name: "GMAIL_DELETE_EMAIL".into(),
            description: "Delete an email".into(),
            required_scope: "admin".into(),
            unlock_paths: vec!["Open Settings > Connections".into()],
        }],
        connected: false,
        non_active_status: Some("INITIATED".into()),
    }];
    let learned = LearnedContextData {
        observations: vec!["observed preference".into()],
        patterns: vec!["pattern one".into()],
        user_profile: vec!["profile fact".into()],
        reflections: vec!["reflection one".into()],
        tree_root_summaries: vec![NamespaceSummary {
            namespace: "activities".into(),
            body: "root memory summary".into(),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("timestamp"),
        }],
    };
    let ctx = PromptContext {
        workspace_dir: workspace.path(),
        model_name: "agentic-v1",
        agent_id: "planner",
        tools: &prompt_tools,
        skills: &skills,
        dispatcher_instructions: "Use tool calls when useful.",
        learned,
        visible_tool_names: &visible_tool_names,
        tool_call_format: ToolCallFormat::PFormat,
        connected_integrations: &integrations,
        connected_identities_md: String::new(),
        include_profile: true,
        include_memory_md: true,
        curated_snapshot: None,
        user_identity: Some(UserIdentity {
            id: Some("user-1".into()),
            name: Some(" Coverage\nUser ".into()),
            email: Some("coverage@example.test".into()),
        }),
        personality_soul_md: None,
        personality_memory_md: None,
        personality_roster: vec![],
    };

    let tools_md = render_tools(&ctx).expect("render tools");
    assert!(tools_md.contains("plan_exit[plan]"));
    assert!(!tools_md.contains("Parameters:"));
    let ambient = render_ambient_environment(&ctx).expect("ambient");
    assert!(ambient.contains("Model: agentic-v1"));
    assert!(ambient.contains("- name: Coverage User"));
    assert!(ambient.contains("Current Date & Time"));

    let built = SystemPromptBuilder::for_subagent(
        "You are a narrow coverage sub-agent.".into(),
        false,
        false,
        true,
    )
    .build(&ctx)
    .expect("subagent builder");
    assert!(built.contains("coverage soul"));
    assert!(built.contains("coverage profile"));
    assert!(built.contains("Output style"));

    let narrow = render_subagent_system_prompt(
        workspace.path(),
        "agentic-v1",
        &[0, 99],
        &tools,
        &[],
        "Subagent archetype body",
        SubagentRenderOptions {
            include_safety_preamble: true,
            include_identity: true,
            include_skills_catalog: false,
            include_profile: true,
            include_memory_md: true,
        },
        ToolCallFormat::Json,
        &integrations,
    );
    assert!(narrow.contains("Subagent archetype body"));
    assert!(narrow.contains("coverage identity"));
    assert!(narrow.contains("Parameters:"));
    assert!(narrow.contains("Do not exfiltrate private data"));

    let native = render_subagent_system_prompt(
        workspace.path(),
        "agentic-v1",
        &[0],
        &tools,
        &[],
        "Native body",
        SubagentRenderOptions::narrow(),
        ToolCallFormat::Native,
        &[],
    );
    assert!(!native.contains("## Tools"));
    assert!(native.contains("native tool-calling output"));
    assert!(UserIdentity::default().is_empty());
    assert!(PromptTool::new("x", "desc").parameters_schema.is_none());
    assert!(PromptTool::with_schema("x", "desc", "{}".into())
        .parameters_schema
        .is_some());
    let options = SubagentRenderOptions::from_definition_flags(false, true, false, true, false);
    assert!(options.include_identity);
    assert!(!options.include_safety_preamble);
    assert!(options.include_skills_catalog);
    assert!(!options.include_profile);
    assert!(options.include_memory_md);
}

fn test_device(total_ram_gb: u64) -> DeviceProfile {
    DeviceProfile {
        total_ram_bytes: total_ram_gb * 1024 * 1024 * 1024,
        cpu_count: 4,
        cpu_brand: "coverage cpu".into(),
        os_name: "coverage os".into(),
        os_version: "1.0".into(),
        has_gpu: false,
        gpu_description: None,
    }
}
