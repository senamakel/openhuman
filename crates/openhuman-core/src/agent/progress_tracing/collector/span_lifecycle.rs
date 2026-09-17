//! Turn-span bootstrap, parent resolution, and tool content capture — the
//! shared span-lifecycle helpers used while folding progress events.

use std::collections::BTreeMap;

use crate::agent::progress_tracing::serialize::{truncate_capture_text, MAX_TOOL_CONTENT_CHARS};
use crate::agent::progress_tracing::types::SpanKind;

use super::state::SpanCollector;

impl SpanCollector {
    /// Lazily open the root turn span so a stream that begins mid-flight
    /// (or never sends `TurnStarted`) still produces a correlated tree.
    pub(in crate::agent::progress_tracing) fn ensure_turn_span(
        &mut self,
        start_unix_ms: u64,
    ) -> String {
        if let Some(id) = &self.turn_span_id {
            return id.clone();
        }
        let mut attrs = BTreeMap::new();
        attrs.insert(
            "session.id".to_string(),
            serde_json::Value::String(self.ctx.session_id.clone()),
        );
        if let Some(user) = &self.ctx.user_id {
            attrs.insert(
                "user.id".to_string(),
                serde_json::Value::String(user.clone()),
            );
        }
        if let Some(client) = &self.ctx.client_id {
            attrs.insert(
                "client.id".to_string(),
                serde_json::Value::String(client.clone()),
            );
        }
        if let Some(agent) = &self.ctx.agent_id {
            attrs.insert(
                "agent.id".to_string(),
                serde_json::Value::String(agent.clone()),
            );
        }
        if let Some(source) = &self.ctx.channel_source {
            attrs.insert(
                "channel.source".to_string(),
                serde_json::Value::String(source.clone()),
            );
        }
        attrs.insert(
            "run.type".to_string(),
            serde_json::Value::String(self.ctx.run_type.as_str().to_string()),
        );
        // Every trace must end up with a Langfuse sessionId: prefer the
        // explicit grouping key (thread/conversation id), else fall back to
        // the trace id itself so the trace is never left session-less.
        let group = self
            .ctx
            .session_group
            .clone()
            .unwrap_or_else(|| self.ctx.session_id.clone());
        attrs.insert("thread.id".to_string(), serde_json::Value::String(group));
        // Trace/root-span name carries agent attribution when known.
        let name = match &self.ctx.agent_id {
            Some(agent) => format!("agent.turn:{agent}"),
            None => "agent.turn".to_string(),
        };
        log::debug!(
            "[agent-tracing] opening turn span trace_id={} name={} user_attributed={} client_attributed={} source={:?}",
            self.ctx.session_id,
            name,
            self.ctx.user_id.is_some(),
            self.ctx.client_id.is_some(),
            self.ctx.channel_source,
        );
        let (id, index) = self.open_span(SpanKind::Turn, name, None, start_unix_ms, attrs);
        self.turn_span_id = Some(id.clone());
        self.turn_span_index = Some(index);
        id
    }

    /// The parent any iteration / tool / subagent span should hang off:
    /// the current iteration if one is open, else the turn root.
    pub(in crate::agent::progress_tracing) fn active_parent_id(
        &mut self,
        now_unix_ms: u64,
    ) -> String {
        if let Some(id) = &self.current_iteration_span_id {
            return id.clone();
        }
        self.ensure_turn_span(now_unix_ms)
    }

    /// Record a tool call's arguments as the span's `input`, truncated to
    /// [`MAX_TOOL_CONTENT_CHARS`]. A no-op unless content capture is on
    /// (`observability.agent_tracing.capture_content`) — when off, tool I/O
    /// never even reaches the in-memory span. `Null` arguments are skipped.
    pub(in crate::agent::progress_tracing) fn capture_tool_arguments(
        &mut self,
        index: usize,
        arguments: &serde_json::Value,
    ) {
        if !self.ctx.capture_content || arguments.is_null() {
            return;
        }
        let serialized = arguments.to_string();
        let chars = serialized.chars().count();
        if let Some(span) = self.spans.get_mut(index) {
            span.input = Some(serde_json::Value::String(truncate_capture_text(
                &serialized,
            )));
            log::trace!(
                "[agent-tracing] captured tool input span={} chars={chars} truncated={}",
                span.name,
                chars > MAX_TOOL_CONTENT_CHARS,
            );
        }
    }

    /// Record a tool call's result as the span's `output`, truncated to
    /// [`MAX_TOOL_CONTENT_CHARS`]. Same capture gate as
    /// [`Self::capture_tool_arguments`]. Empty output is skipped.
    pub(in crate::agent::progress_tracing) fn capture_tool_output(
        &mut self,
        index: usize,
        output: &str,
    ) {
        if !self.ctx.capture_content || output.is_empty() {
            return;
        }
        let chars = output.chars().count();
        if let Some(span) = self.spans.get_mut(index) {
            span.output = Some(serde_json::Value::String(truncate_capture_text(output)));
            log::trace!(
                "[agent-tracing] captured tool output span={} chars={chars} truncated={}",
                span.name,
                chars > MAX_TOOL_CONTENT_CHARS,
            );
        }
    }

    pub(in crate::agent::progress_tracing) fn close_current_iteration(&mut self, end_unix_ms: u64) {
        if let Some(index) = self.current_iteration_index.take() {
            self.close_span(
                index,
                end_unix_ms,
                crate::agent::progress_tracing::types::SpanStatus::Ok,
                BTreeMap::new(),
            );
        }
        self.current_iteration_span_id = None;
    }
}
