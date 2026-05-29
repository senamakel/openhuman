//! Focused raw/E2E coverage for inference and agent controller paths.
//!
//! The suite uses only temp workspaces and loopback HTTP mocks. It avoids live
//! model/provider calls while still exercising the public controller registry.

use std::collections::{HashMap, HashSet};
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
use openhuman_core::openhuman::agent::profiles::{
    AgentProfile, AgentProfileStore, AgentProfilesState, DEFAULT_PROFILE_ID,
};
use openhuman_core::openhuman::agent::task_board::{
    TaskApprovalMode, TaskBoard, TaskBoardCard, TaskBoardStore, TaskCardStatus,
};
use openhuman_core::openhuman::agent::task_dispatcher::build_task_prompt;
use openhuman_core::openhuman::agent::{
    all_agent_controller_schemas, all_agent_registered_controllers,
};
use openhuman_core::openhuman::config::Config;
use openhuman_core::openhuman::credentials::{AuthService, APP_SESSION_PROVIDER};
use openhuman_core::openhuman::inference::context_window_for_model;
use openhuman_core::openhuman::inference::sentiment::local_ai_analyze_sentiment;
use openhuman_core::openhuman::inference::{
    all_inference_controller_schemas, all_inference_registered_controllers,
};

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
