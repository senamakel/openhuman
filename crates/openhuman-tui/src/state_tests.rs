use super::*;

const CLIENT: &str = "tui-abc123";

fn ev(event: &str) -> WebChannelEvent {
    WebChannelEvent {
        event: event.to_string(),
        client_id: CLIENT.to_string(),
        thread_id: "thread-1".to_string(),
        ..Default::default()
    }
}

fn text_delta(delta: &str) -> WebChannelEvent {
    WebChannelEvent {
        delta: Some(delta.to_string()),
        delta_kind: Some("text".to_string()),
        ..ev("text_delta")
    }
}

fn thinking_delta(delta: &str) -> WebChannelEvent {
    WebChannelEvent {
        delta: Some(delta.to_string()),
        delta_kind: Some("thinking".to_string()),
        ..ev("thinking_delta")
    }
}

#[test]
fn text_deltas_accumulate_into_one_assistant_entry() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("hi");
    s.apply_event(&text_delta("Hel"));
    s.apply_event(&text_delta("lo "));
    s.apply_event(&text_delta("world"));

    let assistant: Vec<_> = s
        .entries()
        .iter()
        .filter(|e| e.kind == EntryKind::Assistant)
        .collect();
    assert_eq!(assistant.len(), 1, "deltas must fold into a single entry");
    assert_eq!(assistant[0].text, "Hello world");
    assert!(s.is_streaming(), "still streaming before chat_done");
}

#[test]
fn thinking_and_text_are_separate_entries() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("q");
    s.apply_event(&thinking_delta("let me think"));
    s.apply_event(&text_delta("answer"));

    let kinds: Vec<_> = s.entries().iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![EntryKind::User, EntryKind::Thinking, EntryKind::Assistant]
    );
    let thinking = s
        .entries()
        .iter()
        .find(|e| e.kind == EntryKind::Thinking)
        .unwrap();
    assert_eq!(thinking.text, "let me think");
}

#[test]
fn thinking_deltas_accumulate_separately_from_text() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("q");
    s.apply_event(&thinking_delta("a"));
    s.apply_event(&thinking_delta("b"));
    s.apply_event(&text_delta("x"));
    s.apply_event(&thinking_delta("c")); // interleaved — same thinking entry
    let thinking: Vec<_> = s
        .entries()
        .iter()
        .filter(|e| e.kind == EntryKind::Thinking)
        .collect();
    assert_eq!(thinking.len(), 1);
    assert_eq!(thinking[0].text, "abc");
}

#[test]
fn chat_done_replaces_streamed_text_with_full_response() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("hi");
    s.apply_event(&text_delta("Hel")); // partial / laggy stream
    let done = WebChannelEvent {
        full_response: Some("Hello, world!".to_string()),
        ..ev("chat_done")
    };
    s.apply_event(&done);

    let assistant: Vec<_> = s
        .entries()
        .iter()
        .filter(|e| e.kind == EntryKind::Assistant)
        .collect();
    assert_eq!(assistant.len(), 1);
    assert_eq!(
        assistant[0].text, "Hello, world!",
        "full_response is authoritative and replaces the streamed text"
    );
    assert!(!s.is_streaming(), "chat_done ends the turn");
}

#[test]
fn chat_done_without_prior_deltas_still_shows_full_response() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("hi");
    let done = WebChannelEvent {
        full_response: Some("Direct answer".to_string()),
        ..ev("chat_done")
    };
    s.apply_event(&done);
    let assistant = s
        .entries()
        .iter()
        .find(|e| e.kind == EntryKind::Assistant)
        .expect("chat_done with full_response must produce an assistant entry");
    assert_eq!(assistant.text, "Direct answer");
    assert!(!s.is_streaming());
}

#[test]
fn chat_error_pushes_error_entry_and_ends_stream() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("hi");
    let err = WebChannelEvent {
        message: Some("rate limited".to_string()),
        error_type: Some("rate_limit".to_string()),
        ..ev("chat_error")
    };
    s.apply_event(&err);
    let error = s
        .entries()
        .iter()
        .find(|e| e.kind == EntryKind::Error)
        .expect("chat_error must produce an error entry");
    assert_eq!(error.text, "rate limited");
    assert!(!s.is_streaming(), "chat_error ends the turn");
}

#[test]
fn retryable_error_includes_retry_timing() {
    let mut state = TranscriptState::new(CLIENT);
    state.begin_user_turn("try");
    state.apply_event(&WebChannelEvent {
        message: Some("rate limited".into()),
        error_retryable: Some(true),
        error_retry_after_ms: Some(2_100),
        ..ev("chat_error")
    });
    assert_eq!(
        state.entries().last().unwrap().text,
        "rate limited · retryable after 3s"
    );
}

#[test]
fn last_assistant_excludes_an_in_flight_partial_answer() {
    let mut state = TranscriptState::new(CLIENT);
    state.begin_user_turn("question");
    state.apply_event(&text_delta("partial"));
    assert_eq!(state.last_assistant(), None);
    state.apply_event(&WebChannelEvent {
        full_response: Some("complete".into()),
        ..ev("chat_done")
    });
    assert_eq!(state.last_assistant(), Some("complete"));
}

#[test]
fn events_for_other_client_id_are_ignored() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("hi");
    let before = s.entries().len();
    let foreign = WebChannelEvent {
        client_id: "tui-someone-else".to_string(),
        delta: Some("not mine".to_string()),
        ..WebChannelEvent {
            event: "text_delta".to_string(),
            thread_id: "thread-1".to_string(),
            ..Default::default()
        }
    };
    s.apply_event(&foreign);
    assert_eq!(
        s.entries().len(),
        before,
        "foreign client_id events must not mutate our transcript"
    );
}

#[test]
fn events_for_an_inactive_thread_are_ignored() {
    let mut state = TranscriptState::new(CLIENT);
    state.set_thread("thread-1");
    let event = WebChannelEvent {
        thread_id: "thread-2".into(),
        ..text_delta("not this conversation")
    };
    state.apply_event(&event);
    assert!(state.entries().is_empty());
}

#[test]
fn projected_transcript_is_restored_in_chronological_order() {
    let mut state = TranscriptState::new(CLIENT);
    state.load_transcript(&serde_json::json!({
        "result": {"data": {"items": [
            {"kind": "assistantMessage", "content": "answer"},
            {"kind": "userMessage", "content": "question"}
        ]}}
    }));
    assert_eq!(state.entries()[0].text, "question");
    assert_eq!(state.entries()[1].text, "answer");
}

#[test]
fn tool_call_and_result_produce_tool_entries() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("do it");
    let call = WebChannelEvent {
        tool_name: Some("web_search".to_string()),
        args: Some(serde_json::json!({"query": "rust ratatui"})),
        ..ev("tool_call")
    };
    s.apply_event(&call);
    let result = WebChannelEvent {
        tool_name: Some("web_search".to_string()),
        success: Some(true),
        output: Some("3 results".to_string()),
        ..ev("tool_result")
    };
    s.apply_event(&result);

    let tools: Vec<_> = s
        .entries()
        .iter()
        .filter(|e| e.kind == EntryKind::Tool)
        .collect();
    assert_eq!(tools.len(), 2);
    assert!(tools[0].text.starts_with("→ web_search"));
    assert!(tools[0].text.contains("rust ratatui"));
    assert!(tools[1].text.starts_with("✓ web_search"));
    assert!(tools[1].text.contains("3 results"));
}

#[test]
fn failed_tool_result_uses_cross_marker() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("do it");
    let result = WebChannelEvent {
        tool_name: Some("run_shell".to_string()),
        success: Some(false),
        output: Some("exit code 1".to_string()),
        ..ev("tool_result")
    };
    s.apply_event(&result);
    let tool = s
        .entries()
        .iter()
        .find(|e| e.kind == EntryKind::Tool)
        .unwrap();
    assert!(tool.text.starts_with("✗ run_shell"));
}

#[test]
fn second_turn_opens_fresh_assistant_entry() {
    let mut s = TranscriptState::new(CLIENT);
    s.begin_user_turn("first");
    s.apply_event(&text_delta("one"));
    s.apply_event(&WebChannelEvent {
        full_response: Some("one".to_string()),
        ..ev("chat_done")
    });
    s.begin_user_turn("second");
    s.apply_event(&text_delta("two"));

    let assistant: Vec<_> = s
        .entries()
        .iter()
        .filter(|e| e.kind == EntryKind::Assistant)
        .collect();
    assert_eq!(assistant.len(), 2, "each turn gets its own assistant entry");
    assert_eq!(assistant[0].text, "one");
    assert_eq!(assistant[1].text, "two");
}

#[test]
fn truncate_line_collapses_newlines_and_caps_length() {
    let long = "a\nb\n".repeat(200);
    let out = truncate_line(&long);
    assert!(!out.contains('\n'));
    assert!(out.chars().count() <= 121, "capped to MAX + ellipsis");
}
