//! End-to-end test of the Claude Code stream-json pipeline.
//!
//! Feeds a captured representative CC 2.x stream-json transcript through
//! `StreamJsonParser` → `EventMapper` and asserts that:
//! - text deltas arrive in order and aggregate into the final response
//! - a `tool_use` block is the CLI's **own, already-executed** call: its
//!   `input_json_delta`s are kept out of the visible text and nothing is
//!   surfaced to the harness — no `ToolCallStart`, no `ToolCallArgsDelta`,
//!   no aggregated `ToolCall` (see the `event_mapper` module docs; #5739)
//! - the `result` event finalizes usage tokens (incl. cache_read)
//! - session_id is captured from the first `system` event
//!
//! This is a parser-level E2E; the real driver / process spawn is mocked
//! in `tests/claude_code_driver_smoke.rs`.

use openhuman_core::inference::provider::claude_code::{
    event_mapper::EventMapper, stream_parser::StreamJsonParser,
};
use openhuman_core::inference::provider::types::ProviderDelta;

const TRANSCRIPT: &str = r#"{"type":"system","subtype":"init","session_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","schema_version":"2.0"}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text"}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call_42","name":"memory_search"}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"que"}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"ry\":\"foo\"}"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":1}}
{"type":"assistant","message":{"type":"message","role":"assistant","content":[]}}
{"type":"result","subtype":"success","usage":{"input_tokens":120,"output_tokens":42,"cache_read_input_tokens":80,"cache_creation_input_tokens":0},"total_cost_usd":0.0012}
"#;

#[test]
fn captures_text_tool_call_and_usage() {
    let mut parser = StreamJsonParser::new();
    let mut mapper = EventMapper::new();
    let mut deltas: Vec<ProviderDelta> = Vec::new();

    // Feed in chunks to exercise the chunk-boundary buffering as well.
    let mid = TRANSCRIPT.len() / 2;
    for chunk in [&TRANSCRIPT[..mid], &TRANSCRIPT[mid..]] {
        for evt in parser.feed(chunk) {
            for d in mapper.handle(evt) {
                deltas.push(d);
            }
        }
    }
    for evt in parser.end() {
        for d in mapper.handle(evt) {
            deltas.push(d);
        }
    }

    // Schema version was captured by the parser.
    assert_eq!(parser.schema_version.as_deref(), Some("2.0"));

    // Session id was captured by the mapper from the first system event.
    assert_eq!(
        mapper.session_id.as_deref(),
        Some("f47ac10b-58cc-4372-a567-0e02b2c3d479")
    );

    // Text deltas arrived in order.
    let text_chunks: Vec<&str> = deltas
        .iter()
        .filter_map(|d| match d {
            ProviderDelta::TextDelta { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_chunks, vec!["Hello", " world"]);

    // The CLI's own tool call is suppressed end to end: nothing about
    // `call_42` reaches the harness, and its argument JSON never leaks into
    // the text stream.
    assert!(
        !deltas.iter().any(|d| matches!(
            d,
            ProviderDelta::ToolCallStart { .. } | ProviderDelta::ToolCallArgsDelta { .. }
        )),
        "a self-executed CLI tool_use block must not surface as a harness tool call: {deltas:?}"
    );
    assert!(
        !text_chunks
            .iter()
            .any(|t| t.contains("que") || t.contains("ry\"")),
        "input_json_delta text must stay out of the visible text: {text_chunks:?}"
    );

    // Aggregated response.
    assert_eq!(mapper.final_text, "Hello world");
    assert!(
        mapper.tool_calls.is_empty(),
        "no ToolCall is aggregated for a CLI-internal tool_use block"
    );

    // Usage from the `result` event.
    assert!(mapper.finished);
    let u = mapper.usage.as_ref().expect("usage should be populated");
    assert_eq!(u.input_tokens, 120);
    assert_eq!(u.output_tokens, 42);
    assert_eq!(u.cached_input_tokens, 80);
}
