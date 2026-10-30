use super::*;
use serde_json::json;

fn view(mut event: Value) -> WebEventView {
    let obj = event.as_object_mut().unwrap();
    obj.entry("client_id").or_insert(json!("c1"));
    obj.entry("thread_id").or_insert(json!("t1"));
    obj.entry("request_id").or_insert(json!("r1"));
    obj.entry("seq").or_insert(json!(7));
    let parsed: WebChannelEvent = serde_json::from_value(event).expect("valid event");
    WebEventView::from_event(&parsed)
}

#[test]
fn context_and_text_fields_are_projected() {
    let v = view(json!({"event": "text_delta", "delta": "hi"}));
    assert_eq!(v.event, "text_delta");
    assert_eq!(
        (v.client_id.as_str(), v.thread_id.as_str(), v.request_id.as_str()),
        ("c1", "t1", "r1")
    );
    assert_eq!(v.seq, Some(7));
    assert_eq!(v.delta.as_deref(), Some("hi"));
}

#[test]
fn final_response_events_expose_full_response() {
    for name in ["chat_done", "chat_interim", "proactive_message", "voice_speak"] {
        let event = serde_json::from_value::<WebChannelEvent>(json!({
            "event": name, "client_id": "c", "thread_id": "t", "request_id": "r",
            "full_response": "answer"
        }))
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(WebEventView::from_event(&event).full_response.as_deref(), Some("answer"));
    }
    let v = view(json!({"event": "channel_outbound", "full_response": "sent"}));
    assert_eq!(v.full_response.as_deref(), Some("sent"));
}

#[test]
fn chat_errors_and_cancellation_expose_messages() {
    let v = view(json!({
        "event": "chat_error", "message": "boom", "error_type": "network",
        "error_retryable": true, "error_retry_after_ms": 1500
    }));
    assert_eq!(v.message.as_deref(), Some("boom"));
    assert_eq!(v.error_type.as_deref(), Some("network"));
    assert_eq!(v.error_retryable, Some(true));
    assert_eq!(v.error_retry_after_ms, Some(1500));
    let v = view(json!({"event": "chat_cancelled", "cancel_reason": "user"}));
    assert_eq!(v.message.as_deref(), Some("user"));
    let v = view(json!({"event": "user_error", "message": "bad input", "error_type": "invalid"}));
    assert_eq!(v.message.as_deref(), Some("bad input"));
    assert_eq!(v.error_type.as_deref(), Some("invalid"));
}

#[test]
fn tool_events_project_only_fields_in_the_current_contract() {
    let call = view(json!({
        "event": "tool_call", "tool_name": "read", "args": {"path": "a.rs"},
        "tool_call_id": "tc"
    }));
    assert_eq!(call.tool_name.as_deref(), Some("read"));
    assert_eq!(call.args, Some(json!({"path": "a.rs"})));
    assert_eq!(call.tool_call_id.as_deref(), Some("tc"));
    let result = view(json!({
        "event": "tool_result", "tool_name": "read", "output": "source",
        "success": true, "tool_call_id": "tc", "elapsed_ms": 9
    }));
    assert_eq!(result.tool_name.as_deref(), Some("read"));
    assert_eq!(result.output.as_deref(), Some("source"));
    assert_eq!(result.success, Some(true));
    assert_eq!(result.tool_call_id.as_deref(), Some("tc"));
    assert_eq!(result.elapsed_ms, Some(9));
}

#[test]
fn approval_and_plan_review_events_project_prompt_fields() {
    for name in ["approval_request", "plan_review_request"] {
        let v = view(json!({
            "event": name, "tool_name": "shell", "args": {"cmd": "ls"},
            "tool_call_id": "tc", "message": "allow?"
        }));
        assert_eq!(v.tool_name.as_deref(), Some("shell"), "{name}");
        assert_eq!(v.message.as_deref(), Some("allow?"), "{name}");
        assert_eq!(v.args, Some(json!({"cmd": "ls"})), "{name}");
    }
}

#[test]
fn artifact_events_expose_call_id_and_args() {
    for name in ["artifact_ready", "artifact_failed", "artifact_pending"] {
        let v = view(json!({"event": name, "tool_call_id": "tc", "args": {"a": 1}}));
        assert_eq!(v.tool_call_id.as_deref(), Some("tc"), "{name}");
        assert_eq!(v.args, Some(json!({"a": 1})), "{name}");
    }
}

#[test]
fn desktop_events_leave_transcript_fields_empty() {
    let v = view(json!({"event": "memory_activity", "args": {"type": "recall"}}));
    assert_eq!(v.event, "memory_activity");
    assert!(v.delta.is_none() && v.message.is_none() && v.subagent.is_none());
}
