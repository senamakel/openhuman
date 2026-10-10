use super::*;
use serde_json::json;
use std::sync::Mutex;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn body(content: &str, finish_reason: &str) -> Value {
    json!({
        "id": "gen-1",
        "model": "vendor/answered",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "cost": 0.0125,
            "prompt_tokens_details": {"cached_tokens": 40},
            "completion_tokens_details": {"reasoning_tokens": 5}
        }
    })
}

async fn server_replying(reply: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply))
        .mount(&server)
        .await;
    server
}

fn completer(server: &MockServer) -> Completer {
    Completer::new(Route::openai_compatible(
        format!("{}/v1", server.uri()),
        "sk-test",
    ))
    .header("X-Title", "embed-test")
}

async fn sent_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.expect("recording on");
    assert_eq!(requests.len(), 1);
    serde_json::from_slice(&requests[0].body).unwrap()
}

#[tokio::test]
async fn structured_round_trip_reports_finish_model_usage_and_cost() {
    let server = server_replying(body("{\"prime\":true}", "stop")).await;
    let schema = json!({"type": "object", "properties": {"prime": {"type": "boolean"}}});
    let request = CompletionRequest::new(
        "vendor/requested",
        vec![
            ChatMessage::system("answer in json"),
            ChatMessage::user("is 7 prime?").with_image("data:image/png;base64,AAAA"),
        ],
    )
    .response_format(ResponseFormat::JsonSchema {
        name: "answer".into(),
        schema,
    })
    .max_tokens(64)
    .provider_options(json!({"reasoning": {"effort": "low"}, "usage": {"include": true}}));

    let response = completer(&server).complete(request).await.unwrap();

    assert_eq!(response.structured, Some(json!({"prime": true})));
    assert_eq!(response.finish_reason.as_deref(), Some("stop"));
    assert_eq!(response.answered_model.as_deref(), Some("vendor/answered"));
    let usage = response.usage.expect("usage reported");
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 20);
    assert_eq!(usage.cost_usd, Some(0.0125));

    let sent = sent_body(&server).await;
    assert_eq!(sent["model"], "vendor/requested");
    assert_eq!(sent["max_tokens"], 64);
    assert_eq!(sent["reasoning"]["effort"], "low");
    assert_eq!(sent["response_format"]["type"], "json_schema");
    assert!(sent.get("tools").is_none());
    let received = server.received_requests().await.unwrap();
    assert_eq!(
        received[0]
            .headers
            .get("x-title")
            .map(|v| v.to_str().unwrap()),
        Some("embed-test")
    );
    let parts = sent["messages"][1]["content"]
        .as_array()
        .expect("multipart user message");
    assert!(
        parts.iter().any(|part| part["type"] == "image_url"),
        "image part forwarded: {parts:?}"
    );
}

#[tokio::test]
async fn truncated_json_is_reported_not_parsed() {
    let server = server_replying(body("{\"prime\":", "length")).await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")])
        .response_format(ResponseFormat::JsonObject);
    let error = completer(&server).complete(request).await.unwrap_err();
    assert!(
        matches!(error, CoreError::StructuredOutput { failure, .. } if failure.reason == crate::structured::StructuredFailureReason::Truncated)
    );
}

#[tokio::test]
async fn text_format_never_parses_json() {
    let server = server_replying(body("{\"a\":1}", "stop")).await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")]);
    let response = completer(&server).complete(request).await.unwrap();
    assert_eq!(response.structured, None);
}

#[tokio::test]
async fn cleartext_bearer_to_remote_host_is_refused() {
    let completer = Completer::new(Route::openai_compatible("http://api.example.com/v1", "sk"));
    let err = completer
        .complete(CompletionRequest::new("m", vec![ChatMessage::user("x")]))
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::InsecureRoute { .. }), "{err:?}");
}

#[tokio::test]
async fn blank_model_is_refused() {
    let completer = Completer::new(Route::openai_compatible("https://api.example.com/v1", "sk"));
    let err = completer
        .complete(CompletionRequest::new("  ", vec![ChatMessage::user("x")]))
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidRoute { .. }), "{err:?}");
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(bool, String)>>);

impl CompletionObserver for Recorder {
    fn on_complete(&self, trace: &CompletionTrace<'_>) {
        self.0
            .lock()
            .unwrap()
            .push((trace.outcome.is_ok(), trace.request.model.clone()));
    }
}

#[tokio::test]
async fn observer_fires_once_per_call_on_success_and_error() {
    let server = server_replying(body("hi", "stop")).await;
    let recorder = Arc::new(Recorder::default());
    let ok = completer(&server).observer(recorder.clone());
    ok.complete(CompletionRequest::new("good", vec![ChatMessage::user("x")]))
        .await
        .unwrap();
    let bad = Completer::new(Route::openai_compatible("http://api.example.com/v1", "sk"))
        .observer(recorder.clone());
    let _ = bad
        .complete(CompletionRequest::new("bad", vec![ChatMessage::user("x")]))
        .await;
    assert_eq!(
        *recorder.0.lock().unwrap(),
        vec![(true, "good".to_string()), (false, "bad".to_string())]
    );
}

#[test]
fn json_reply_tolerates_one_code_fence() {
    assert_eq!(
        parse_json_reply("```json\n{\"a\":1}\n```"),
        Some(json!({"a": 1}))
    );
    assert_eq!(
        parse_json_reply("```\n{\"a\":1}\n```"),
        Some(json!({"a": 1}))
    );
    assert_eq!(parse_json_reply("  {\"a\":1} "), Some(json!({"a": 1})));
    assert_eq!(parse_json_reply("not json"), None);
}

#[test]
fn data_uri_mime_is_extracted() {
    assert_eq!(
        data_uri_mime("data:image/png;base64,AA").as_deref(),
        Some("image/png")
    );
    assert_eq!(data_uri_mime("https://x/y.png"), None);
}

#[test]
fn completer_debug_redacts_the_bearer() {
    let completer = Completer::new(Route::openai_compatible("https://h/v1", "sk-secret"));
    assert!(!format!("{completer:?}").contains("sk-secret"));
}

#[tokio::test]
async fn json_object_format_rejects_a_non_object_reply() {
    let server = server_replying(body("[1,2]", "stop")).await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")])
        .response_format(ResponseFormat::JsonObject);
    let error = completer(&server).complete(request).await.unwrap_err();
    assert!(
        matches!(error, CoreError::StructuredOutput { failure, .. } if failure.reason == crate::structured::StructuredFailureReason::SchemaMismatch)
    );
}

#[tokio::test]
async fn json_object_format_keeps_an_object_reply() {
    let server = server_replying(body("{\"ok\":true}", "stop")).await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")])
        .response_format(ResponseFormat::JsonObject);
    let response = completer(&server).complete(request).await.unwrap();
    assert_eq!(response.structured, Some(json!({"ok": true})));
}

#[tokio::test]
async fn raw_cost_survives_a_usage_block_without_tokens() {
    let mut reply = body("hi", "stop");
    reply["usage"] = json!({"cost": 0.5});
    let server = server_replying(reply).await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")]);
    let response = completer(&server).complete(request).await.unwrap();
    let usage = response.usage.expect("the provider reported a cost");
    assert_eq!(usage.cost_usd, Some(0.5));
}

#[tokio::test]
async fn provider_failure_maps_to_an_rpc_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
        .mount(&server)
        .await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")]);
    let err = completer(&server)
        .complete(request)
        .await
        .expect_err("a 400 must surface as an error");
    assert!(
        matches!(err, CoreError::Rpc { method, .. } if method == COMPLETE),
        "{err:?}"
    );
}

#[tokio::test]
async fn slow_provider_hits_the_timeout_branch() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(body("late", "stop"))
                .set_delay(Duration::from_millis(2_000)),
        )
        .mount(&server)
        .await;
    let request = CompletionRequest::new("m", vec![ChatMessage::user("x")]);
    let err = completer(&server)
        .timeout(Duration::from_millis(50))
        .complete(request)
        .await
        .expect_err("the call must time out before the delayed reply");
    match err {
        CoreError::DeadlineExceeded { method } => assert_eq!(method, COMPLETE),
        other => panic!("expected a typed deadline error, got {other:?}"),
    }
}
