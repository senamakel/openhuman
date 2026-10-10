//! Journal-projection regressions from production trace d1f1562f (a
//! `workflow_builder` sub-agent trace): completions whose `ModelStarted` was
//! lost, tools that never completed, unlabeled models, failed runs, and the
//! turn input.

use super::*;
use tinyagents_harness::terminal::{TerminalOutcome, TerminalReason};

fn run_started() -> AgentObservation {
    obs(
        0,
        1_000,
        AgentEvent::RunStarted {
            run_id: RunId::new("run-1"),
            thread_id: None,
        },
    )
}

fn model_started(offset: u64, ts: u64, call: &str) -> AgentObservation {
    obs(
        offset,
        ts,
        AgentEvent::ModelStarted {
            call_id: CallId::new(call),
            model: "openrouter/deepseek/deepseek-v4-flash".to_string(),
        },
    )
}

fn model_completed(
    offset: u64,
    ts: u64,
    call: &str,
    started: u64,
    input: Option<serde_json::Value>,
) -> AgentObservation {
    obs(
        offset,
        ts,
        AgentEvent::ModelCompleted {
            call_id: CallId::new(call),
            started_at_ms: Some(started),
            usage: Some(Usage::new(100, 10)),
            input,
            output: None,
        },
    )
}

fn tool_started(offset: u64, ts: u64, call: &str, name: &str) -> AgentObservation {
    obs(
        offset,
        ts,
        AgentEvent::ToolStarted {
            parent_call_id: None,
            call_id: CallId::new(call),
            tool_name: name.to_string(),
            input: None,
        },
    )
}

fn run_completed(offset: u64, ts: u64) -> AgentObservation {
    obs(
        offset,
        ts,
        AgentEvent::RunCompleted {
            run_id: RunId::new("run-1"),
            outcome: None,
        },
    )
}

fn generations(spans: &[super::super::TraceSpan]) -> Vec<&super::super::TraceSpan> {
    spans
        .iter()
        .filter(|s| s.kind == SpanKind::Generation)
        .collect()
}

#[test]
fn completion_without_model_started_gets_its_own_iteration_and_start() {
    let observations = vec![
        run_started(),
        model_started(1, 1_000, "m1"),
        model_completed(2, 3_000, "m1", 1_000, None),
        // `ModelStarted` for m2 / m3 lost from the journal.
        model_completed(3, 5_000, "m2", 3_100, None),
        model_completed(4, 9_000, "m3", 5_200, None),
        run_completed(5, 9_100),
    ];
    let spans = spans_from_observations(ctx(), 10, &observations);
    let gens = generations(&spans);
    let timing: Vec<(u64, Option<u64>)> = gens
        .iter()
        .map(|g| (g.start_unix_ms, g.end_unix_ms))
        .collect();
    assert_eq!(
        timing,
        vec![
            (1_000, Some(3_000)),
            (3_100, Some(5_000)),
            (5_200, Some(9_000))
        ],
        "each call starts at its own started_at_ms"
    );
    for g in &gens {
        assert_eq!(g.name, "llm.openrouter/deepseek/deepseek-v4-flash");
        assert_eq!(
            g.attributes["gen_ai.request.model"],
            serde_json::json!("openrouter/deepseek/deepseek-v4-flash"),
            "no empty provider prefix or empty model"
        );
    }
    let iterations = spans
        .iter()
        .filter(|s| s.kind == SpanKind::Iteration)
        .count();
    assert_eq!(
        iterations, 3,
        "a lost ModelStarted still yields its iteration"
    );
}

#[test]
fn tool_that_never_completed_is_force_closed_at_its_iteration_end() {
    let observations = vec![
        run_started(),
        model_started(1, 1_000, "m1"),
        model_completed(2, 2_000, "m1", 1_000, None),
        tool_started(3, 2_010, "t1", "get_node_kind_contract"),
        // ToolCompleted for t1 lost; the next model call starts at 2_500.
        model_started(4, 2_500, "m2"),
        model_completed(5, 3_000, "m2", 2_500, None),
        run_completed(6, 400_000),
    ];
    let spans = spans_from_observations(ctx(), 10, &observations);
    let tool = spans
        .iter()
        .find(|s| s.name == "tool.get_node_kind_contract")
        .unwrap();
    assert_eq!(tool.end_unix_ms, Some(2_500));
    assert_eq!(tool.attributes["force_closed"], serde_json::json!(true));
    assert_eq!(
        tool.attributes["observation.level"],
        serde_json::json!("WARNING")
    );
}

#[test]
fn top_level_run_failure_marks_the_turn_span() {
    let observations = vec![
        run_started(),
        model_started(1, 1_000, "m1"),
        obs(
            2,
            2_000,
            AgentEvent::RunFailed {
                run_id: RunId::new("run-1"),
                error: "provider unavailable".to_string(),
                outcome: None,
            },
        ),
    ];
    let spans = spans_from_observations(ctx(), 10, &observations);
    let turn = spans.iter().find(|s| s.kind == SpanKind::Turn).unwrap();
    assert_eq!(turn.status, SpanStatus::Error);
    assert_eq!(turn.attributes["turn.outcome"], serde_json::json!("failed"));
    assert_eq!(turn.end_unix_ms, Some(2_000));
}

#[test]
fn cancelled_run_is_a_warning() {
    let observations = vec![
        run_started(),
        obs(
            1,
            2_000,
            AgentEvent::RunFailed {
                run_id: RunId::new("run-1"),
                error: "cancelled".to_string(),
                outcome: Some(TerminalOutcome::new(TerminalReason::Cancelled, "cancelled")),
            },
        ),
    ];
    let spans = spans_from_observations(ctx(), 10, &observations);
    let turn = spans.iter().find(|s| s.kind == SpanKind::Turn).unwrap();
    assert_eq!(turn.status, SpanStatus::Unset);
    assert_eq!(
        turn.attributes["turn.outcome"],
        serde_json::json!("cancelled")
    );
}

#[test]
fn turn_input_is_the_user_message_not_the_last_call_input() {
    let observations = vec![
        run_started(),
        model_started(1, 1_000, "m1"),
        model_completed(
            2,
            2_000,
            "m1",
            1_000,
            Some(serde_json::json!([
                {"system": {"content": [{"text": "You are OpenHuman."}]}},
                {"user": {"content": [{"text": "build a daily digest"}]}}
            ])),
        ),
        model_started(3, 2_100, "m2"),
        model_completed(
            4,
            3_000,
            "m2",
            2_100,
            Some(serde_json::json!([
                {"user": {"content": [{"text": "build a daily digest"}]}},
                {"assistant": {"content": [{"text": "calling tools"}]}},
                {"user": {"content": [{"text": "[Tool results]\\n{}"}]}}
            ])),
        ),
        run_completed(5, 3_100),
    ];
    let spans = spans_from_observations(ctx().with_capture_content(true), 10, &observations);
    let turn = spans.iter().find(|s| s.kind == SpanKind::Turn).unwrap();
    assert_eq!(
        turn.input.as_ref().unwrap(),
        &serde_json::json!("build a daily digest")
    );
}
