//! Trace accuracy regressions from production Langfuse traces (Oct 2026):
//! per-call generation start, unstreamed TTFT, late/missing tool completions,
//! turn outcome, turn input, model labels, unpriced cost and usage totals.

use super::*;

use tinyagents_harness::observability::trace_export::otlp::otlp_requests;

fn iteration(iteration: u32) -> AgentProgress {
    AgentProgress::IterationStarted {
        iteration,
        max_iterations: 15,
    }
}

#[allow(clippy::too_many_arguments)]
fn call(
    model: &str,
    provider: &str,
    task: Option<&str>,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cost: f64,
) -> AgentProgress {
    AgentProgress::ModelCallCompleted {
        model: model.to_string(),
        provider_id: provider.to_string(),
        subagent_task_id: task.map(str::to_string),
        input: None,
        output: None,
        iteration: 1,
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: cache_read,
        cache_creation_tokens: cache_write,
        reasoning_tokens: 0,
        cost_usd: cost,
    }
}

fn simple_call(model: &str) -> AgentProgress {
    call(model, "managed", None, 100, 10, 0, 0, 0.001)
}

fn child_iteration(task: &str, iteration: u32) -> AgentProgress {
    AgentProgress::SubagentIterationStarted {
        agent_id: "researcher".to_string(),
        task_id: task.to_string(),
        iteration,
        max_iterations: 10,
        extended_policy: false,
    }
}

fn child_tool_started(task: &str, call_id: &str, tool: &str) -> AgentProgress {
    AgentProgress::SubagentToolCallStarted {
        agent_id: "researcher".to_string(),
        task_id: task.to_string(),
        call_id: call_id.to_string(),
        tool_name: tool.to_string(),
        arguments: serde_json::Value::Null,
        iteration: 1,
        display_label: None,
        display_detail: None,
    }
}

fn child_tool_completed(task: &str, call_id: &str, tool: &str, elapsed: u64) -> AgentProgress {
    AgentProgress::SubagentToolCallCompleted {
        agent_id: "researcher".to_string(),
        task_id: task.to_string(),
        call_id: call_id.to_string(),
        tool_name: tool.to_string(),
        success: true,
        output_chars: 3,
        output: String::new(),
        arguments: None,
        elapsed_ms: elapsed,
        iteration: 1,
        failure: None,
        display_label: None,
        display_detail: None,
        structured: None,
    }
}

fn child_completed(task: &str, elapsed: u64) -> AgentProgress {
    AgentProgress::SubagentCompleted {
        agent_id: "researcher".to_string(),
        task_id: task.to_string(),
        elapsed_ms: elapsed,
        iterations: 1,
        output_chars: 0,
        output: String::new(),
        usage: None,
        worktree_path: None,
        changed_files: Vec::new(),
        dirty_status: None,
        stop: None,
    }
}

fn generations(spans: &[TraceSpan]) -> Vec<&TraceSpan> {
    spans
        .iter()
        .filter(|span| span.kind == SpanKind::Generation)
        .collect()
}

fn exported_attr(spans: &[TraceSpan], name: &str, key: &str) -> Option<String> {
    let payloads = otlp_requests(spans, "production", &super::export_brand());
    payloads.iter().find_map(|payload| {
        payload["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()?
            .iter()
            .find(|span| span["name"] == name)?["attributes"]
            .as_array()?
            .iter()
            .find(|item| item["key"] == key)
            .and_then(|item| item["value"]["stringValue"].as_str())
            .map(str::to_string)
    })
}

// ── 1. each model call starts at its own request start ──────────────────────

#[test]
fn calls_folded_into_one_iteration_do_not_share_its_start() {
    // Production d1f1562f: four generations in one iteration all started at
    // the iteration's first millisecond, so each latency covered the calls
    // before it. A later call cannot start before the previous one ended.
    let mut c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (simple_call("chat-v1"), 3_000),
        (simple_call("chat-v1"), 5_000),
        (simple_call("chat-v1"), 9_000),
    ]);
    c.finish(9_500);
    let spans = c.spans();
    let starts: Vec<(u64, Option<u64>)> = generations(spans)
        .iter()
        .map(|g| (g.start_unix_ms, g.end_unix_ms))
        .collect();
    assert_eq!(
        starts,
        vec![
            (1_000, Some(3_000)),
            (3_000, Some(5_000)),
            (5_000, Some(9_000))
        ]
    );
}

#[test]
fn explicit_call_start_wins() {
    let mut c = SpanCollector::new(ctx());
    c.record(&AgentProgress::TurnStarted, 1_000);
    c.record(&iteration(1), 1_000);
    c.set_next_call_start(None, 2_500);
    c.record(&simple_call("chat-v1"), 3_000);
    assert_eq!(generations(c.spans())[0].start_unix_ms, 2_500);
}

// ── 2. TTFT is not recorded for a non-streamed call ─────────────────────────

#[test]
fn single_synthetic_delta_at_completion_records_no_ttft() {
    // The OpenAI Responses path is unary and emits the whole reply as one
    // delta right before completing; its "first token" is the latency.
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (
            AgentProgress::TextDelta {
                delta: "whole reply".into(),
                iteration: 1,
            },
            8_995,
        ),
        (simple_call("gpt-6.1-sol"), 9_000),
    ]);
    let g = generations(c.spans())[0];
    assert!(!g
        .attributes
        .contains_key("gen_ai.response.first_token_unix_ms"));
    assert!(!g
        .attributes
        .contains_key("gen_ai.response.time_to_first_token_ms"));
    assert_eq!(
        g.attributes["gen_ai.response.streamed"],
        serde_json::json!(false)
    );
    assert_eq!(
        exported_attr(
            c.spans(),
            "llm.gpt-6.1-sol",
            "langfuse.observation.completion_start_time"
        ),
        None
    );
}

// ── 3. sub-agent tool completions and force-closed spans ────────────────────

#[test]
fn late_child_tool_completion_after_subagent_completed_closes_the_span() {
    // Under channel backpressure the child bridge's completion can reach the
    // collector after the orchestrator's `SubagentCompleted`; it used to be
    // dropped, leaving the tool open until the turn ended (407 s spans).
    let mut c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (spawn("t1", "Builder"), 1_100),
        (child_iteration("t1", 1), 1_200),
        (
            child_tool_started("t1", "c1", "get_node_kind_contract"),
            1_300,
        ),
        (child_completed("t1", 900), 2_000),
        (
            child_tool_completed("t1", "c1", "get_node_kind_contract", 4),
            2_050,
        ),
        (
            AgentProgress::TurnCompleted {
                iterations: 1,
                stop: None,
            },
            400_000,
        ),
    ]);
    c.finish(400_000);
    let tool = find(c.spans(), "tool.get_node_kind_contract");
    assert_eq!(tool.end_unix_ms, Some(1_304));
    assert_eq!(tool.status, SpanStatus::Ok);
    assert!(!tool.attributes.contains_key("force_closed"));
}

#[test]
fn late_child_model_call_nests_under_its_subagent() {
    let mut c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (spawn("t1", "Builder"), 1_100),
        (child_iteration("t1", 1), 1_200),
        (child_completed("t1", 900), 2_000),
        (call("", "", Some("t1"), 10, 1, 0, 0, 0.0), 2_050),
    ]);
    c.finish(3_000);
    let spans = c.spans();
    let child_iter = find(spans, "subagent.iteration#1");
    let g = generations(spans)[0];
    assert_eq!(
        g.parent_span_id.as_deref(),
        Some(child_iter.span_id.as_str()),
        "a late child call must not land in the parent's iteration"
    );
}

#[test]
fn tool_without_completion_is_force_closed_at_its_parent_end() {
    let mut c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (tool_started("c1", "composio_list_toolkits", 1), 1_500),
        (iteration(2), 2_000),
        (simple_call("chat-v1"), 3_000),
        (
            AgentProgress::TurnCompleted {
                iterations: 2,
                stop: None,
            },
            440_000,
        ),
    ]);
    c.finish(440_000);
    let tool = find(c.spans(), "tool.composio_list_toolkits");
    assert_eq!(
        tool.end_unix_ms,
        Some(2_000),
        "bounded by the iteration end, not the turn end"
    );
    assert_eq!(tool.attributes["force_closed"], serde_json::json!(true));
    assert_eq!(
        tool.attributes["observation.level"],
        serde_json::json!("WARNING")
    );
    assert_eq!(tool.status, SpanStatus::Unset);
}

// ── 4. the root turn span reflects the outcome ──────────────────────────────

#[test]
fn turn_without_completion_is_an_error() {
    let mut c = collect(&[(AgentProgress::TurnStarted, 1_000), (iteration(1), 1_000)]);
    c.finish(5_000);
    let turn = find(c.spans(), "agent.turn");
    assert_eq!(turn.status, SpanStatus::Error);
    assert_eq!(
        turn.attributes["turn.outcome"],
        serde_json::json!("incomplete")
    );
    assert_eq!(
        exported_attr(c.spans(), "agent.turn", "langfuse.observation.level").as_deref(),
        Some("ERROR")
    );
    assert!(exported_attr(
        c.spans(),
        "agent.turn",
        "langfuse.observation.status_message"
    )
    .is_some());
}

#[test]
fn failed_turn_message_is_content_gated() {
    let failed = || TurnOutcome::Failed {
        message: "provider rejected /Users/alice/secret.txt".into(),
    };
    let mut gated = collect(&[(AgentProgress::TurnStarted, 1_000)]);
    gated.finish_with_outcome(2_000, Some(failed()));
    let turn = find(gated.spans(), "agent.turn");
    assert_eq!(turn.status, SpanStatus::Error);
    assert_eq!(
        turn.attributes["error.message"],
        serde_json::json!("Turn failed")
    );

    let mut captured = collect_with_capture(&[(AgentProgress::TurnStarted, 1_000)]);
    captured.finish_with_outcome(2_000, Some(failed()));
    let turn = find(captured.spans(), "agent.turn");
    assert!(turn.attributes["error.message"]
        .as_str()
        .unwrap()
        .contains("provider rejected"));
}

#[test]
fn cancelled_turn_is_a_warning_not_an_error() {
    let mut c = collect(&[(AgentProgress::TurnStarted, 1_000)]);
    c.finish_with_outcome(2_000, Some(TurnOutcome::Cancelled { reason: None }));
    let turn = find(c.spans(), "agent.turn");
    assert_eq!(turn.status, SpanStatus::Unset);
    assert_eq!(
        turn.attributes["turn.outcome"],
        serde_json::json!("cancelled")
    );
    assert_eq!(
        turn.attributes["observation.level"],
        serde_json::json!("WARNING")
    );
}

#[test]
fn completed_turn_is_not_reclassified_by_finish() {
    let mut c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (
            AgentProgress::TurnCompleted {
                iterations: 1,
                stop: None,
            },
            2_000,
        ),
    ]);
    c.finish_with_outcome(
        3_000,
        Some(TurnOutcome::Failed {
            message: "x".into(),
        }),
    );
    let turn = find(c.spans(), "agent.turn");
    assert_eq!(turn.status, SpanStatus::Ok);
    assert_eq!(turn.end_unix_ms, Some(2_000));
}

// ── 5. the turn input is the user's message ─────────────────────────────────

#[test]
fn first_turn_input_is_kept() {
    let c = collect_with_capture(&[
        (AgentProgress::TurnStarted, 1_000),
        (
            AgentProgress::TurnContent {
                input: Some("what's on my calendar?".into()),
                output: None,
            },
            1_000,
        ),
        (
            AgentProgress::TurnContent {
                input: Some("[Tool results]\n…".into()),
                output: Some("You have two meetings.".into()),
            },
            5_000,
        ),
    ]);
    let turn = find(c.spans(), "agent.turn");
    assert_eq!(
        turn.input.as_ref().unwrap(),
        &serde_json::json!("what's on my calendar?")
    );
    assert_eq!(
        turn.output.as_ref().unwrap(),
        &serde_json::json!("You have two meetings.")
    );
}

// ── 6. model labels ─────────────────────────────────────────────────────────

#[test]
fn empty_provider_and_model_do_not_produce_dot_labels() {
    let mut c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (
            call(
                "openrouter/deepseek/deepseek-v4-flash",
                "",
                None,
                10,
                1,
                0,
                0,
                0.0,
            ),
            2_000,
        ),
        (iteration(2), 3_000),
        (call("", "", None, 10, 1, 0, 0, 0.0), 4_000),
    ]);
    c.finish(4_000);
    for g in generations(c.spans()) {
        assert_eq!(g.name, "llm.openrouter/deepseek/deepseek-v4-flash");
        assert_eq!(
            g.attributes["gen_ai.request.model"],
            serde_json::json!("openrouter/deepseek/deepseek-v4-flash")
        );
    }
}

// ── 7. unpriced / free models ───────────────────────────────────────────────

#[test]
fn unpriced_model_placeholder_cost_is_not_recorded() {
    let model = "acme/never-heard-of-it";
    let placeholder = crate::agent::cost::estimate_call_cost_usd(
        model,
        &crate::inference::provider::BilledUsage::from_counts(1_000, 100),
    );
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (
            call(model, "openrouter", None, 1_000, 100, 0, 0, placeholder),
            2_000,
        ),
    ]);
    let g = generations(c.spans())[0];
    assert_eq!(
        g.attributes["gen_ai.usage.cost_usd"],
        serde_json::json!(0.0)
    );
    assert_eq!(
        g.attributes["gen_ai.cost.source"],
        serde_json::json!("unpriced")
    );
    assert!(!g
        .attributes
        .contains_key("gen_ai.pricing.input_per_mtok_usd"));
    assert_eq!(
        exported_attr(
            c.spans(),
            "llm.acme/never-heard-of-it",
            "langfuse.observation.cost_details"
        ),
        None,
        "no fabricated costDetails reach Langfuse"
    );
}

#[test]
fn free_model_is_priced_at_zero() {
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (
            call(
                "deepseek/deepseek-chat-v3.1:free",
                "openrouter",
                None,
                1_000,
                100,
                0,
                0,
                0.0,
            ),
            2_000,
        ),
    ]);
    let g = generations(c.spans())[0];
    assert_eq!(
        g.attributes["gen_ai.usage.cost_usd"],
        serde_json::json!(0.0)
    );
    assert_eq!(
        g.attributes["gen_ai.pricing.output_per_mtok_usd"],
        serde_json::json!(0.0)
    );
}

// ── 8. usage totals are consistent across routes ────────────────────────────

#[test]
fn uncached_only_input_route_reports_a_consistent_total() {
    // Claude Code route: raw Anthropic `input_tokens` (uncached remainder).
    let c = collect(&[
        (AgentProgress::TurnStarted, 1_000),
        (iteration(1), 1_000),
        (
            call(
                "claude-sonnet-4-5",
                "claude-code",
                None,
                12,
                300,
                40_000,
                2_000,
                0.0,
            ),
            2_000,
        ),
    ]);
    let g = generations(c.spans())[0];
    let a = &g.attributes;
    assert_eq!(a["gen_ai.usage.input_tokens"], serde_json::json!(42_012));
    assert_eq!(
        a["gen_ai.usage.uncached_input_tokens"],
        serde_json::json!(12)
    );
    assert_eq!(
        a["gen_ai.usage.cache_creation_tokens"],
        serde_json::json!(2_000)
    );
    assert_eq!(a["gen_ai.usage.total_tokens"], serde_json::json!(42_312));
    let usage: serde_json::Value = serde_json::from_str(
        &exported_attr(
            c.spans(),
            "llm.claude-sonnet-4-5",
            "langfuse.observation.usage_details",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(usage["total"], serde_json::json!(42_312));
    assert!(usage["total"].as_u64().unwrap() >= usage["cache_read_input_tokens"].as_u64().unwrap());
}
