//! [`SpanCollector`] data shape, construction, and small accessors.

use std::collections::BTreeMap;

use crate::agent::progress_tracing::types::{SpanStatus, TraceContext, TraceSpan};

/// Per-subagent bookkeeping so child iterations / tool calls nest correctly.
#[derive(Debug)]
pub(super) struct SubagentState {
    /// Index of the subagent span in [`SpanCollector::spans`].
    pub(super) span_index: usize,
    /// Currently-open child iteration span id, if any.
    pub(super) current_iteration_span_id: Option<String>,
    /// Open child tool spans keyed by `call_id` → span index.
    pub(super) open_tools: BTreeMap<String, usize>,
}

/// Pure state machine that folds an [`crate::agent::progress::AgentProgress`]
/// stream into spans.
///
/// Call [`record`](Self::record) for each event (with a millisecond
/// timestamp), then [`finish`](Self::finish) once the stream closes to seal
/// any still-open spans. [`spans`](Self::spans) returns the accumulated tree.
#[derive(Debug)]
pub struct SpanCollector {
    pub(super) ctx: TraceContext,
    pub(super) spans: Vec<TraceSpan>,
    pub(super) next_span_seq: u64,
    /// Per-collector (per-turn) random prefix for minted span ids. Langfuse
    /// dedupes observations by id **globally**, so a bare per-turn sequence
    /// (`0000…0001`) collides across turns and silently binds later turns'
    /// observations to whichever trace first claimed the id. Prefixing with a
    /// fresh nonce makes every span id globally unique.
    pub(super) id_prefix: String,

    pub(super) turn_span_id: Option<String>,
    pub(super) turn_span_index: Option<usize>,
    pub(super) current_iteration_span_id: Option<String>,
    pub(super) current_iteration_index: Option<usize>,

    /// Open parent-turn tool spans keyed by `call_id` → span index.
    pub(super) open_tools: BTreeMap<String, usize>,
    /// Live subagents keyed by `task_id`.
    pub(super) subagents: BTreeMap<String, SubagentState>,
}

impl SpanCollector {
    pub fn new(ctx: TraceContext) -> Self {
        Self {
            ctx,
            spans: Vec::new(),
            next_span_seq: 0,
            id_prefix: uuid::Uuid::new_v4().simple().to_string(),
            turn_span_id: None,
            turn_span_index: None,
            current_iteration_span_id: None,
            current_iteration_index: None,
            open_tools: BTreeMap::new(),
            subagents: BTreeMap::new(),
        }
    }

    /// Opt into attaching content to spans (prompt/reply, generation
    /// request/completion, tool + subagent I/O). Wire this to
    /// `observability.agent_tracing.capture_content`. Equivalent to setting
    /// [`TraceContext::with_capture_content`] before construction — there is a
    /// single storage-level gate (`ctx.capture_content`), so content dropped
    /// here can never reach any exporter.
    pub fn with_content_capture(mut self, capture_content: bool) -> Self {
        self.ctx.capture_content = capture_content;
        self
    }

    /// All spans recorded so far (finished and in-flight).
    pub fn spans(&self) -> &[TraceSpan] {
        &self.spans
    }

    /// Index of the span with `span_id`, if any.
    pub(super) fn span_index_by_id(&self, span_id: &str) -> Option<usize> {
        self.spans.iter().position(|sp| sp.span_id == span_id)
    }

    /// Consume the collector and return its spans.
    #[cfg(test)]
    pub fn into_spans(self) -> Vec<TraceSpan> {
        self.spans
    }

    /// OTel-style 16-hex span id derived from a monotonic sequence. Stable
    /// and deterministic within a run, which keeps the tests reproducible.
    pub(super) fn mint_span_id(&mut self) -> String {
        self.next_span_seq += 1;
        // Nonce prefix keeps the id globally unique across turns (Langfuse
        // dedupes observations by id project-wide).
        format!("{}-{:016x}", self.id_prefix, self.next_span_seq)
    }

    pub(super) fn open_span(
        &mut self,
        kind: crate::agent::progress_tracing::types::SpanKind,
        name: impl Into<String>,
        parent_span_id: Option<String>,
        start_unix_ms: u64,
        attributes: BTreeMap<String, serde_json::Value>,
    ) -> (String, usize) {
        let span_id = self.mint_span_id();
        let index = self.spans.len();
        self.spans.push(TraceSpan {
            trace_id: self.ctx.session_id.clone(),
            span_id: span_id.clone(),
            parent_span_id,
            name: name.into(),
            kind,
            start_unix_ms,
            end_unix_ms: None,
            status: SpanStatus::Unset,
            attributes,
            input: None,
            output: None,
        });
        (span_id, index)
    }

    /// Seal a span: set its end timestamp + status and merge in any extra
    /// attributes. A no-op if `index` is out of range (defensive).
    pub(super) fn close_span(
        &mut self,
        index: usize,
        end_unix_ms: u64,
        status: SpanStatus,
        extra: BTreeMap<String, serde_json::Value>,
    ) {
        if let Some(span) = self.spans.get_mut(index) {
            // Don't let a late event drag end before start.
            span.end_unix_ms = Some(end_unix_ms.max(span.start_unix_ms));
            span.status = status;
            span.attributes.extend(extra);
        }
    }
}
