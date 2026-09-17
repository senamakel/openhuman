use super::*;
use std::collections::BTreeMap;

use crate::agent::progress_tracing::SpanKind;
use tinyagents_harness::ids::{CallId, EventId, RunId};
use tinyinference::usage::Usage;

fn span(
    trace: &str,
    id: &str,
    parent: Option<&str>,
    name: &str,
    kind: SpanKind,
    status: SpanStatus,
    start: u64,
    end: Option<u64>,
) -> TraceSpan {
    let mut attributes = BTreeMap::new();
    attributes.insert("tokens".to_string(), json!(42));
    TraceSpan {
        trace_id: trace.to_string(),
        span_id: id.to_string(),
        parent_span_id: parent.map(str::to_string),
        name: name.to_string(),
        kind,
        start_unix_ms: start,
        end_unix_ms: end,
        status,
        attributes,
        input: None,
        output: None,
    }
}

fn obs(offset: u64, event: AgentEvent) -> AgentObservation {
    AgentObservation {
        event_id: EventId::new(format!("run-1-evt-{offset}")),
        run_id: RunId::new("run-1"),
        parent_run_id: None,
        root_run_id: RunId::new("run-1"),
        offset,
        ts_ms: 1_000 + offset,
        event,
    }
}

#[path = "langfuse_batch_tests.rs"]
mod batch_tests;
#[path = "langfuse_trace_fields_tests.rs"]
mod trace_fields_tests;
