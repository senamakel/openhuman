//! Two users, one thread id, two conversations: SaaS profiles in-process.
//!
//! [`ProfileRuntime`] boots a core in SaaS mode on a scratch root, provisions
//! a profile for `alice` and one for `bob`, and has each say something on a
//! thread both call `t1`. Each then reads `t1` back and sees only their own
//! messages: a thread id is unique per profile, never per process.
//!
//! It runs offline. Managed inference goes to a local mock that answers
//! `echo: <your message>`, reached through a stub backend transport, because
//! a profile's config names no inference endpoint of its own; a real host
//! installs the TinyHumans transport (`openhuman_tinyhumans::install`) and
//! hands each profile its user's credential instead.
//!
//! ```bash
//! cargo run -p openhuman-embed --example profiles
//! ```
//!
//! Building a `ProfileRuntime` locks the process into SaaS mode: no
//! `Runtime::builder()` core can boot beside it.

use std::sync::Arc;

use openhuman_core::core::runtime::{AGENT_WORKER_STACK_BYTES, MAX_BLOCKING_THREADS};
use openhuman_embed::profiles::ProfileCredentialKind;
use openhuman_embed::{
    BackendRequest, BackendTransport, BackendTransportError, BaseUrlPurpose, ProfileRuntime,
    SaasConfig, TransportProfile,
};
use serde_json::{json, Value};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();
    // A turn's async state machine overflows tokio's default worker stack;
    // see `run_turn.rs`.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(AGENT_WORKER_STACK_BYTES)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .build()?;
    // Run on a worker thread, whose stack is the tuned one.
    runtime.block_on(async { tokio::spawn(run()).await? })
}

async fn run() -> anyhow::Result<()> {
    // ── offline inference ────────────────────────────────────────────────
    let inference = MockServer::start().await;
    Mock::given(wiremock::matchers::path_regex(r"chat/completions$"))
        .respond_with(Echo)
        .mount(&inference)
        .await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
        .with_priority(10)
        .mount(&inference)
        .await;
    openhuman_embed::install_backend_transport(Arc::new(MockTransport(inference.uri())));

    // ── the SaaS core ────────────────────────────────────────────────────
    // The root must be absolute, existing and not world-writable; a temp
    // directory is all three. `build` writes a service token there, since
    // nothing serves a gateway here.
    let root = tempfile::tempdir()?;
    let profiles = ProfileRuntime::build(SaasConfig::new(root.path())).await?;
    println!("SaaS root: {}", root.path().display());

    // ── one profile per user ─────────────────────────────────────────────
    let mut handles = Vec::new();
    for (user, said) in [
        ("alice", "Remind me to water the ferns."),
        ("bob", "What rhymes with orange?"),
    ] {
        let provisioned = profiles.provision(user).await?;
        profiles
            .set_credential(
                &provisioned.profile_id,
                ProfileCredentialKind::ApiKey,
                &format!("{user}-demo-key"),
            )
            .await?;
        let handle = profiles.open(user).await?;
        let reply = handle.chat("t1", said).await?;
        println!(
            "\n[{user}] sent message to t1; got reply ({} bytes)",
            reply.text.len()
        );
        handles.push((user, handle));
    }

    // ── each sees only their own t1 ──────────────────────────────────────
    for (user, handle) in &handles {
        let messages = handle.messages("t1").await?;
        println!(
            "\n{user}'s t1 ({}) has {} messages",
            handle.workspace_dir().display(),
            messages.len()
        );
        for (i, message) in messages.iter().enumerate() {
            println!("  [{i}] {}", message.sender);
        }
    }
    println!("\nprofiles: {:?}", profiles.list().await?);

    drop(handles);
    profiles.shutdown().await;
    Ok(())
}

/// Answers every chat completion with `echo: <the last user message>`,
/// streamed when the request asks for it.
struct Echo;

impl Respond for Echo {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let said = body["messages"]
            .as_array()
            .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
            .and_then(|m| m["content"].as_str())
            .unwrap_or_default();
        // The core prefixes the turn's message with context lines (the date);
        // the user's own text is the last line.
        let said = said
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default();
        let reply = format!("echo: {said}");
        if body["stream"] == true {
            let chunk = json!({ "id": "c", "object": "chat.completion.chunk", "created": 0,
                "model": "echo", "choices": [{ "index": 0,
                "delta": { "role": "assistant", "content": reply }, "finish_reason": null }] });
            let done = json!({ "id": "c", "object": "chat.completion.chunk", "created": 0,
                "model": "echo", "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] });
            return ResponseTemplate::new(200).set_body_raw(
                format!("data: {chunk}\n\ndata: {done}\n\ndata: [DONE]\n\n"),
                "text/event-stream",
            );
        }
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "c", "object": "chat.completion", "created": 0, "model": "echo",
            "choices": [{ "index": 0, "finish_reason": "stop",
                "message": { "role": "assistant", "content": reply } }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
        }))
    }
}

/// Sends managed inference to the mock; every other backend call answers
/// "no backend".
struct MockTransport(String);

#[async_trait::async_trait]
impl BackendTransport for MockTransport {
    async fn send_json(&self, _req: BackendRequest<'_>) -> Result<Value, BackendTransportError> {
        Err(BackendTransportError::Unavailable)
    }
    async fn send_multipart(
        &self,
        _req: BackendRequest<'_>,
        _form: reqwest::multipart::Form,
    ) -> Result<Value, BackendTransportError> {
        Err(BackendTransportError::Unavailable)
    }
    fn http_client(&self, _profile: TransportProfile) -> reqwest::Client {
        reqwest::Client::new()
    }
    fn base_url(&self, _configured: Option<&str>, _purpose: BaseUrlPurpose) -> String {
        self.0.clone()
    }
    fn product_identity(&self) -> String {
        "openhuman-embed-example".to_string()
    }
    fn attribution_headers(&self) -> reqwest::header::HeaderMap {
        reqwest::header::HeaderMap::new()
    }
    fn name(&self) -> &'static str {
        "example-mock"
    }
}
