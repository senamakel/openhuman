//! Focused raw-line coverage for owned domains with local-only dependencies.
//!
//! This suite drives public APIs for the agent, inference, and composio slices
//! against temp directories and loopback Axum servers. It avoids live providers.

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::tempdir;

use openhuman_core::openhuman::agent::task_board::{
    board_for_thread, TaskApprovalMode, TaskBoard, TaskBoardCard, TaskBoardStore, TaskCardStatus,
};
use openhuman_core::openhuman::composio::ComposioClient;
use openhuman_core::openhuman::inference::provider::compatible::{
    AuthStyle, OpenAiCompatibleProvider,
};
use openhuman_core::openhuman::inference::provider::{
    ChatMessage, ChatRequest, Provider, ProviderDelta,
};
use openhuman_core::openhuman::integrations::IntegrationClient;
use openhuman_core::openhuman::tools::ToolSpec;

#[derive(Clone, Default)]
struct ProviderMockState {
    chat_requests: Arc<Mutex<Vec<Value>>>,
    response_requests: Arc<Mutex<Vec<Value>>>,
    auth_headers: Arc<Mutex<Vec<Option<String>>>>,
    user_agents: Arc<Mutex<Vec<Option<String>>>>,
}

#[derive(Clone, Default)]
struct ComposioMockState {
    requests: Arc<Mutex<Vec<(String, String, Option<Value>, Option<String>)>>>,
}

async fn serve_provider_mock() -> (String, ProviderMockState) {
    let state = ProviderMockState::default();
    let app = Router::new()
        .route("/v1/chat/completions", post(provider_chat))
        .route("/v1/responses", post(provider_responses))
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
    (format!("http://{addr}/v1"), state)
}

async fn provider_chat(
    State(state): State<ProviderMockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state
        .auth_headers
        .lock()
        .expect("auth headers")
        .push(header(&headers, "authorization"));
    state
        .user_agents
        .lock()
        .expect("user agents")
        .push(header(&headers, "user-agent"));
    state
        .chat_requests
        .lock()
        .expect("chat requests")
        .push(body.clone());

    if body.pointer("/stream").and_then(Value::as_bool) == Some(true) {
        return Json(json!({
            "choices": [{
                "message": {
                    "content": "stream fallback body",
                    "reasoning_content": "stream thinking"
                }
            }],
            "usage": {
                "prompt_tokens": 3,
                "completion_tokens": 4,
                "total_tokens": 7,
                "prompt_tokens_details": { "cached_tokens": 2 }
            }
        }))
        .into_response();
    }

    if body.get("tools").is_some() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "unknown parameter: tools" })),
        )
            .into_response();
    }

    if body.pointer("/model").and_then(Value::as_str) == Some("missing-chat") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "chat completions unavailable" })),
        )
            .into_response();
    }

    Json(json!({
        "choices": [{
            "message": {
                "content": "<think>hidden</think>visible answer",
                "reasoning_content": "model reasoning",
                "tool_calls": [{
                    "id": "call-1",
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "arguments": { "query": "openhuman" }
                    }
                }]
            }
        }],
        "openhuman": {
            "usage": {
                "input_tokens": 11,
                "output_tokens": 13,
                "cached_input_tokens": 5
            },
            "billing": { "charged_amount_usd": 0.0123 }
        }
    }))
    .into_response()
}

async fn provider_responses(
    State(state): State<ProviderMockState>,
    Json(body): Json<Value>,
) -> Response {
    state
        .response_requests
        .lock()
        .expect("response requests")
        .push(body);
    Json(json!({
        "output_text": "responses fallback answer",
        "output": []
    }))
    .into_response()
}

async fn serve_composio_mock() -> (String, ComposioMockState) {
    let state = ComposioMockState::default();
    let app = Router::new()
        .route(
            "/agent-integrations/composio/toolkits",
            get(composio_toolkits),
        )
        .route("/agent-integrations/composio/tools", get(composio_tools))
        .route(
            "/agent-integrations/composio/authorize",
            post(composio_authorize),
        )
        .route(
            "/agent-integrations/composio/connections/{id}",
            delete(composio_delete_connection),
        )
        .route(
            "/agent-integrations/composio/triggers/available",
            get(composio_available_triggers),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind composio mock");
    let addr = listener.local_addr().expect("composio mock addr");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("composio mock serve");
    });
    (format!("http://{addr}"), state)
}

async fn composio_toolkits(State(state): State<ComposioMockState>, headers: HeaderMap) -> Response {
    record_composio(
        &state,
        "GET",
        "/agent-integrations/composio/toolkits",
        None,
        &headers,
    );
    Json(json!({ "success": true, "data": { "toolkits": ["gmail", "github"] } })).into_response()
}

async fn composio_tools(
    State(state): State<ComposioMockState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    record_composio(
        &state,
        "GET",
        uri.path_and_query().unwrap().as_str(),
        None,
        &headers,
    );
    Json(json!({
        "success": true,
        "data": {
            "tools": [{
                "type": "function",
                "function": {
                    "name": "GMAIL_SEND_EMAIL",
                    "description": "Send email",
                    "parameters": { "type": "object" }
                }
            }]
        }
    }))
    .into_response()
}

async fn composio_authorize(
    State(state): State<ComposioMockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    record_composio(
        &state,
        "POST",
        "/agent-integrations/composio/authorize",
        Some(body),
        &headers,
    );
    Json(json!({
        "success": true,
        "data": {
            "connectUrl": "https://example.test/connect",
            "connectionId": "conn_123"
        }
    }))
    .into_response()
}

async fn composio_delete_connection(
    State(state): State<ComposioMockState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    record_composio(
        &state,
        "DELETE",
        &format!("/agent-integrations/composio/connections/{id}"),
        None,
        &headers,
    );
    Json(json!({
        "success": true,
        "data": { "deleted": true, "memory_chunks_deleted": 2 }
    }))
    .into_response()
}

async fn composio_available_triggers(
    State(state): State<ComposioMockState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    record_composio(
        &state,
        "GET",
        uri.path_and_query().unwrap().as_str(),
        None,
        &headers,
    );
    Json(json!({
        "success": true,
        "data": {
            "triggers": [{
                "slug": "GMAIL_NEW_GMAIL_MESSAGE",
                "scope": "static",
                "defaultConfig": { "labelIds": ["INBOX"] },
                "requiredConfigKeys": ["labelIds"]
            }]
        }
    }))
    .into_response()
}

fn record_composio(
    state: &ComposioMockState,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &HeaderMap,
) {
    state.requests.lock().expect("composio requests").push((
        method.to_string(),
        path.to_string(),
        body,
        header(headers, "authorization"),
    ));
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

#[tokio::test]
async fn openai_compatible_provider_covers_auth_temperature_tool_fallback_and_responses() {
    let (base_url, state) = serve_provider_mock().await;
    let provider = OpenAiCompatibleProvider::new_with_user_agent(
        "owned-mock",
        &base_url,
        Some("secret-token"),
        AuthStyle::Bearer,
        "OpenHumanOwnedCoverage/1.0",
    )
    .with_temperature_unsupported_models(vec!["gpt-5*".to_string()]);

    let tool = ToolSpec {
        name: "lookup".to_string(),
        description: "Lookup a record".to_string(),
        parameters: json!({ "type": "object" }),
    };
    let messages = vec![ChatMessage::system("system"), ChatMessage::user("hello")];
    let fallback_response = provider
        .chat(
            ChatRequest {
                messages: &messages,
                tools: Some(&[tool.clone(), tool]),
                stream: None,
            },
            "gpt-5-mini",
            0.6,
        )
        .await
        .expect("provider chat with tool fallback");
    assert!(
        fallback_response
            .text
            .as_deref()
            .is_some_and(|text| text.contains("\"tool_calls\"")),
        "tool-schema fallback should return the history-path text payload"
    );
    assert!(fallback_response.tool_calls.is_empty());

    let response = provider
        .chat(
            ChatRequest {
                messages: &messages,
                tools: None,
                stream: None,
            },
            "gpt-5-mini",
            0.6,
        )
        .await
        .expect("provider native chat");
    assert_eq!(response.text.as_deref(), Some("visible answer"));
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "lookup");
    assert_eq!(response.tool_calls[0].arguments, r#"{"query":"openhuman"}"#);
    let usage = response.usage.expect("usage");
    assert_eq!(usage.input_tokens, 11);
    assert_eq!(usage.output_tokens, 13);
    assert_eq!(usage.cached_input_tokens, 5);
    assert_eq!(usage.charged_amount_usd, 0.0123);

    let chat_requests = state.chat_requests.lock().expect("chat requests").clone();
    assert!(
        chat_requests.len() >= 2,
        "tool rejection should force a retry without native tools"
    );
    assert_eq!(
        chat_requests[0].pointer("/tools/0/function/name"),
        Some(&json!("lookup"))
    );
    assert!(chat_requests[0].get("temperature").is_none());
    assert!(chat_requests[1].get("tools").is_none());

    let auth_headers = state.auth_headers.lock().expect("auth headers").clone();
    assert!(
        auth_headers
            .iter()
            .any(|header| header.as_deref() == Some("Bearer secret-token")),
        "bearer auth should be sent"
    );
    let user_agents = state.user_agents.lock().expect("user agents").clone();
    assert!(
        user_agents
            .iter()
            .any(|header| header.as_deref() == Some("OpenHumanOwnedCoverage/1.0")),
        "custom user-agent should be sent"
    );

    let fallback_text = provider
        .chat_with_history(&[ChatMessage::user("fallback please")], "missing-chat", 0.4)
        .await
        .expect("responses fallback");
    assert_eq!(fallback_text, "responses fallback answer");
    assert_eq!(
        state.response_requests.lock().expect("response requests")[0].pointer("/input/0/content"),
        Some(&json!("fallback please"))
    );
}

#[tokio::test]
async fn openai_compatible_provider_streaming_json_fallback_aggregates_response() {
    let (base_url, _state) = serve_provider_mock().await;
    let provider = OpenAiCompatibleProvider::new("owned-mock", &base_url, None, AuthStyle::None);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ProviderDelta>(4);
    let messages = vec![ChatMessage::user("stream please")];

    let response = provider
        .chat(
            ChatRequest {
                messages: &messages,
                tools: None,
                stream: Some(&tx),
            },
            "stream-model",
            0.7,
        )
        .await
        .expect("streaming JSON fallback");

    assert_eq!(response.text.as_deref(), Some("stream fallback body"));
    assert_eq!(
        response.reasoning_content.as_deref(),
        Some("stream thinking")
    );
    assert!(
        rx.try_recv().is_err(),
        "non-SSE fallback should not emit deltas"
    );
}

#[tokio::test]
async fn composio_client_round_trips_backend_paths_and_payload_normalization() {
    let (base_url, state) = serve_composio_mock().await;
    let client = ComposioClient::new(Arc::new(IntegrationClient::new(
        format!("{base_url}/openai/v1/chat/completions"),
        "jwt-token".to_string(),
    )));

    let toolkits = client.list_toolkits().await.expect("toolkits");
    assert_eq!(toolkits.toolkits, vec!["gmail", "github"]);

    let tools = client
        .list_tools(
            Some(&[
                " gmail ".to_string(),
                "".to_string(),
                "github repo".to_string(),
            ]),
            Some(&[" mail ".to_string()]),
        )
        .await
        .expect("tools");
    assert_eq!(tools.tools[0].function.name, "GMAIL_SEND_EMAIL");

    let authorized = client
        .authorize(
            " gmail ",
            Some(json!({
                "oauth_scopes": "profile https://www.googleapis.com/auth/gmail.readonly"
            })),
        )
        .await
        .expect("authorize");
    assert_eq!(authorized.connection_id, "conn_123");

    let triggers = client
        .list_available_triggers(" gmail ", Some("conn 123"))
        .await
        .expect("available triggers");
    assert_eq!(triggers.triggers[0].slug, "GMAIL_NEW_GMAIL_MESSAGE");

    let deleted = client
        .delete_connection(" conn_123 ")
        .await
        .expect("delete connection");
    assert!(deleted.deleted);
    assert_eq!(deleted.memory_chunks_deleted, 2);

    let requests = state.requests.lock().expect("composio requests").clone();
    assert_eq!(requests[0].3.as_deref(), Some("Bearer jwt-token"));
    assert!(
        requests.iter().any(|(_, path, _, _)| path
            == "/agent-integrations/composio/tools?toolkits=gmail,github%20repo&tags=mail"),
        "list_tools should trim blanks and URL-encode query values: {requests:?}"
    );
    let authorize_body = requests
        .iter()
        .find(|(method, path, _, _)| {
            method == "POST" && path == "/agent-integrations/composio/authorize"
        })
        .and_then(|(_, _, body, _)| body.clone())
        .expect("authorize body");
    assert_eq!(authorize_body.get("toolkit"), Some(&json!("gmail")));
    assert_eq!(
        authorize_body.pointer("/oauth_scopes/0"),
        Some(&json!("profile"))
    );
    assert!(authorize_body["oauth_scopes"]
        .as_array()
        .expect("oauth scopes")
        .iter()
        .any(|scope| scope == "https://www.googleapis.com/auth/gmail.readonly"));
    assert!(
        requests.iter().any(|(method, path, _, _)| {
            method == "DELETE" && path == "/agent-integrations/composio/connections/conn_123"
        }),
        "delete_connection should use DELETE route: {requests:?}"
    );
}

#[tokio::test]
async fn agent_task_board_store_normalizes_persists_and_surfaces_errors() {
    let dir = tempdir().expect("tempdir");
    let store = TaskBoardStore::new(dir.path().to_path_buf());

    assert_eq!(TaskCardStatus::InProgress.as_str(), "in_progress");
    assert_eq!(TaskApprovalMode::NotRequired.as_str(), "not_required");
    assert!(store.get(" missing ").expect("get missing").is_none());
    assert!(store
        .get("   ")
        .expect_err("blank id")
        .contains("thread_id"));

    let saved = store
        .put(TaskBoard {
            thread_id: " thread-owned ".to_string(),
            cards: vec![
                TaskBoardCard {
                    id: " ".to_string(),
                    title: "  Draft owned coverage  ".to_string(),
                    status: TaskCardStatus::Blocked,
                    objective: Some("  Raise raw coverage  ".to_string()),
                    plan: vec![
                        " inspect ".to_string(),
                        " ".to_string(),
                        " test ".to_string(),
                    ],
                    assigned_agent: Some(" agent ".to_string()),
                    allowed_tools: vec![" cargo ".to_string(), "".to_string()],
                    approval_mode: Some(TaskApprovalMode::Required),
                    acceptance_criteria: vec![" tests pass ".to_string()],
                    evidence: vec![" coverage measured ".to_string()],
                    notes: Some(" waiting ".to_string()),
                    blocker: None,
                    source_metadata: None,
                    order: 99,
                    updated_at: String::new(),
                },
                TaskBoardCard {
                    id: "drop-me".to_string(),
                    title: "   ".to_string(),
                    status: TaskCardStatus::Todo,
                    objective: None,
                    plan: Vec::new(),
                    assigned_agent: None,
                    allowed_tools: Vec::new(),
                    approval_mode: None,
                    acceptance_criteria: Vec::new(),
                    evidence: Vec::new(),
                    notes: None,
                    blocker: None,
                    source_metadata: None,
                    order: 99,
                    updated_at: String::new(),
                },
            ],
            updated_at: String::new(),
        })
        .expect("put task board");

    assert_eq!(saved.thread_id, "thread-owned");
    assert_eq!(saved.cards.len(), 1);
    assert!(saved.cards[0].id.starts_with("task-"));
    assert_eq!(saved.cards[0].title, "Draft owned coverage");
    assert_eq!(saved.cards[0].plan, vec!["inspect", "test"]);
    assert_eq!(saved.cards[0].blocker.as_deref(), Some("waiting"));
    assert_eq!(saved.cards[0].order, 0);

    let loaded = board_for_thread(dir.path(), " thread-owned ")
        .expect("board_for_thread")
        .cards;
    assert_eq!(loaded[0].approval_mode, Some(TaskApprovalMode::Required));

    assert!(store.delete("thread-owned").expect("delete present"));
    assert!(!store.delete("thread-owned").expect("delete missing"));

    let missing = board_for_thread(dir.path(), "thread-owned").expect("missing board");
    assert!(missing.cards.is_empty());
}
