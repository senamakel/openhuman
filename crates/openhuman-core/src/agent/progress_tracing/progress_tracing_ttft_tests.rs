//! Time to first token on generation spans: when a streamed model call's first
//! delta arrived, relative to the call's start, and its export to Langfuse.

use super::*;

use tinyagents_harness::observability::trace_export::otlp::otlp_requests;
use tinyagents_harness::observability::trace_export::SpanKind;

fn iteration(iteration: u32) -> AgentProgress {
    AgentProgress::IterationStarted {
        iteration,
        max_iterations: 15,
    }
}

fn thinking(iteration: u32) -> AgentProgress {
    AgentProgress::ThinkingDelta {
        delta: "…".to_string(),
        iteration,
    }
}

fn text(iteration: u32) -> AgentProgress {
    AgentProgress::TextDelta {
        delta: "Hi".to_string(),
        iteration,
    }
}

fn completed(iteration: u32) -> AgentProgress {
    AgentProgress::ModelCallCompleted {
        model: "chat-v1".to_string(),
        provider_id: "managed".to_string(),
        subagent_task_id: None,
        input: None,
        output: None,
        iteration,
        input_tokens: 100,
        output_tokens: 10,
        cached_input_tokens: 90,
        cache_creation_tokens: 0,
        reasoning_tokens: 5,
        cost_usd: Some(0.001),
    }
}

fn generations(spans: &[TraceSpan]) -> Vec<&TraceSpan> {
    spans
        .iter()
        .filter(|span| span.kind == SpanKind::Generation)
        .collect()
}

fn attr_u64(span: &TraceSpan, key: &str) -> Option<u64> {
    span.attributes.get(key).and_then(serde_json::Value::as_u64)
}

#[test]
fn streamed_call_records_first_token_and_first_text() {
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (thinking(1), 1_300),
        (thinking(1), 1_400),
        (text(1), 1_800),
        (text(1), 1_900),
        (completed(1), 2_500),
    ]);
    let spans = c.spans();
    let generation = generations(spans)[0];
    assert_eq!(
        attr_u64(generation, "gen_ai.response.first_token_unix_ms"),
        Some(1_300)
    );
    assert_eq!(
        attr_u64(generation, "gen_ai.response.time_to_first_token_ms"),
        Some(300)
    );
    assert_eq!(
        attr_u64(generation, "gen_ai.response.time_to_first_text_ms"),
        Some(800)
    );
}

#[test]
fn unary_call_carries_no_first_token_attributes() {
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (completed(1), 2_500),
    ]);
    let generation = generations(c.spans())[0];
    assert!(!generation
        .attributes
        .contains_key("gen_ai.response.first_token_unix_ms"));
    assert!(!generation
        .attributes
        .contains_key("gen_ai.response.time_to_first_text_ms"));
}

#[test]
fn each_iteration_measures_its_own_first_token() {
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (text(1), 1_200),
        (completed(1), 1_500),
        (iteration(2), 2_000),
        (text(2), 2_700),
        (completed(2), 3_000),
    ]);
    let spans = c.spans();
    let generations = generations(spans);
    assert_eq!(generations.len(), 2);
    assert_eq!(
        attr_u64(generations[0], "gen_ai.response.time_to_first_token_ms"),
        Some(200)
    );
    assert_eq!(
        attr_u64(generations[1], "gen_ai.response.time_to_first_token_ms"),
        Some(700)
    );
}

#[test]
fn otlp_exports_completion_start_time_for_langfuse_ttft() {
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_700_000_000_000),
        (iteration(1), 1_700_000_000_000),
        (text(1), 1_700_000_000_250),
        (completed(1), 1_700_000_001_000),
    ]);
    let spans = c.spans().to_vec();
    let payloads = otlp_requests(
        &spans,
        "production",
        &crate::agent::progress_tracing::export_brand(),
    );
    let exported = payloads[0]["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array()
        .unwrap();
    let generation = exported
        .iter()
        .find(|span| span["name"] == "llm.chat-v1")
        .expect("generation span exported");
    let completion_start = generation["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["key"] == "langfuse.observation.completion_start_time")
        .and_then(|item| item["value"]["stringValue"].as_str());
    assert_eq!(completion_start, Some("2023-11-14T22:13:20.250Z"));
}
