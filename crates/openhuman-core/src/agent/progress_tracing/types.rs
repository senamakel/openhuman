//! Trace/span data model: run classification, trace-level correlation
//! context, and the exported span shape itself.

use std::collections::BTreeMap;

use serde::Serialize;

/// Kind of run a trace belongs to, rendered as stable snake_case strings for
/// Langfuse trace tags (`run:<type>`) and metadata (`run_type`) so runs can be
/// filtered in the UI.
///
/// Only kinds actually observable at the collector installation point (the
/// web progress bridge) exist here: orchestration passes, subconscious runs,
/// cron turns, and meeting agents run their turns WITHOUT a progress bridge
/// today, so they never reach the span collector and get no variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunType {
    /// Interactive user chat turn (desktop UI / socket / PTT / dictation).
    #[default]
    InteractiveChat,
    /// Autonomous background run from the task dispatcher.
    AutonomousTask,
    /// Inbound message relayed from an external channel (Telegram, Discord,
    /// Slack, …) through the channel bus.
    ChannelInbound,
}

impl RunType {
    /// Stable snake_case identifier used in tags/metadata.
    pub fn as_str(self) -> &'static str {
        match self {
            RunType::InteractiveChat => "interactive_chat",
            RunType::AutonomousTask => "autonomous_task",
            RunType::ChannelInbound => "channel_inbound",
        }
    }

    /// Classify from the chat-request `source` tag. Known background sources
    /// map to their kinds; everything else (`ptt`/`dictation`/`type`/absent)
    /// is an interactive chat turn.
    pub fn from_source(source: Option<&str>) -> Self {
        match source {
            Some("autonomous") => RunType::AutonomousTask,
            Some("channel_inbound") => RunType::ChannelInbound,
            _ => RunType::InteractiveChat,
        }
    }
}

/// Trace-level correlation context, stamped onto the root span.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Trace id — unique per turn. Every span of a single turn shares it, so
    /// each turn becomes its own Langfuse trace.
    pub session_id: String,
    /// Real authenticated user attribution (the backend user id, or email as
    /// fallback) — exported as the Langfuse `userId`. `None` when the caller
    /// is anonymous. Transport identifiers (socket client id / "system")
    /// belong in [`Self::client_id`], not here.
    pub user_id: Option<String>,
    /// Transport client id (the broadcast socket client, or `"system"` for
    /// autonomous runs). Exported as the `client.id` metadata attribute so it
    /// stays inspectable without polluting user attribution.
    pub client_id: Option<String>,
    /// Agent definition id driving the turn (e.g. `"orchestrator"`,
    /// `"researcher"`). Stamped as the `agent.id` attribute and folded into
    /// the root span/trace name (`agent.turn:<agent_id>`).
    pub agent_id: Option<String>,
    /// Where the run originated (`"chat"`, `"ptt"`, `"autonomous"`, …).
    /// Exported as the `channel.source` metadata attribute.
    pub channel_source: Option<String>,
    /// Grouping key (the thread/conversation id) exported as the Langfuse
    /// `sessionId` so per-turn traces still group under one session. When
    /// `None`, the collector falls back to the trace id so every trace still
    /// carries a session id.
    pub session_group: Option<String>,
    /// Whether content capture (`observability.agent_tracing.capture_content`)
    /// is on. Gates recording tool arguments/results onto spans at collection
    /// time — when off, tool I/O never even reaches the in-memory span.
    pub capture_content: bool,
    /// Kind of run — exported as Langfuse trace tags (`run:<type>`) and the
    /// `run_type` metadata key. Defaults to interactive chat.
    pub run_type: RunType,
    /// This run's own id (the tinyagents `RunContext` run id), exported as the
    /// `run_id` metadata key. `None` until the run's observations are known.
    pub run_id: Option<String>,
    /// The spawning run's id when this run is a sub-agent/graph node, exported
    /// as the `parent_run_id` metadata key. `None` for top-level turns. This is
    /// what links a spawned sub-agent's trace back to its parent turn (#4657).
    pub parent_run_id: Option<String>,
    /// The root ancestor run id (equal to [`Self::run_id`] for top-level runs),
    /// exported as the `root_run_id` metadata key so Langfuse can thread a whole
    /// spawn tree under one root.
    pub root_run_id: Option<String>,
}

impl TraceContext {
    pub fn new(session_id: impl Into<String>, user_id: Option<String>) -> Self {
        Self {
            session_id: session_id.into(),
            user_id,
            client_id: None,
            agent_id: None,
            channel_source: None,
            session_group: None,
            capture_content: false,
            run_type: RunType::default(),
            run_id: None,
            parent_run_id: None,
            root_run_id: None,
        }
    }

    /// Set the grouping key (thread/conversation id) for the Langfuse
    /// `sessionId`, so a conversation's per-turn traces group together.
    pub fn with_session_group(mut self, group: impl Into<String>) -> Self {
        self.session_group = Some(group.into());
        self
    }

    /// Set the transport client id (`client.id` metadata attribute).
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Set the agent definition id (`agent.id` attribute + trace name suffix).
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// Set the run origin (`channel.source` metadata attribute).
    pub fn with_channel_source(mut self, source: impl Into<String>) -> Self {
        self.channel_source = Some(source.into());
        self
    }

    /// Enable/disable content capture (tool arguments/results on spans).
    pub fn with_capture_content(mut self, capture_content: bool) -> Self {
        self.capture_content = capture_content;
        self
    }

    /// Set the run type (Langfuse `run:<type>` tag / `run_type` metadata).
    pub fn with_run_type(mut self, run_type: RunType) -> Self {
        self.run_type = run_type;
        self
    }

    /// Stamp the run lineage (`run_id` / `parent_run_id` / `root_run_id`) so a
    /// spawned sub-agent's trace links back to its parent turn (#4657). The ids
    /// come from the tinyagents `RunContext`, surfaced via the run's journalled
    /// observations at export time.
    pub fn with_run_lineage(
        mut self,
        run_id: Option<String>,
        parent_run_id: Option<String>,
        root_run_id: Option<String>,
    ) -> Self {
        self.run_id = run_id;
        self.parent_run_id = parent_run_id;
        self.root_run_id = root_run_id;
        self
    }
}

/// Derive the trace id (session id) for a run: prefer the UI session id when
/// present, otherwise fall back to the thread id so headless/autonomous runs
/// (which carry no UI session) still correlate their spans.
pub fn trace_session_id(ui_session_id: Option<u64>, thread_id: &str) -> String {
    ui_session_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| thread_id.to_string())
}

/// What a span represents. Mirrors the [`super::AgentProgress`] lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// The whole turn (root span).
    Turn,
    /// One LLM iteration of the parent turn.
    Iteration,
    /// A tool call.
    Tool,
    /// A single LLM call (model invocation) with per-call usage/cost.
    Generation,
    /// A spawned subagent.
    Subagent,
    /// One LLM iteration inside a subagent.
    SubagentIteration,
}

/// OTel-style span status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    /// Not yet completed, or completed without an explicit success signal.
    Unset,
    /// Completed successfully.
    Ok,
    /// Completed with an error.
    Error,
}

/// A single finished (or in-flight) span. Field names follow OpenTelemetry
/// conventions (snake_case `trace_id`/`span_id`/`start_unix_ms`/…) so the raw
/// NDJSON file/log export is a self-describing OTel-style span dump for local
/// inspection.
///
/// #4469 item 13: this raw record is **not** directly Langfuse-ingestible — the
/// Langfuse `/api/public/ingestion` API needs each span wrapped in a
/// `{ type, id, timestamp, body }` event envelope. That envelope is produced
/// only by [`super::langfuse::spans_to_langfuse_batch`] on the remote-push path; the
/// local NDJSON exporter intentionally emits the raw spans, not the batch
/// format.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSpan {
    /// Trace id (the session id) — shared by every span in the run.
    pub trace_id: String,
    /// Unique id of this span within the trace.
    pub span_id: String,
    /// Parent span id, or `None` for the root turn span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    /// Human-readable span name, e.g. `agent.turn`, `tool.web_search`.
    pub name: String,
    /// Structured kind for programmatic filtering.
    pub kind: SpanKind,
    /// Wall-clock start (Unix epoch milliseconds).
    pub start_unix_ms: u64,
    /// Wall-clock end (Unix epoch milliseconds); `None` while in flight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_unix_ms: Option<u64>,
    /// Completion status.
    pub status: SpanStatus,
    /// Metadata-only attributes (no secrets/PII).
    pub attributes: BTreeMap<String, serde_json::Value>,
    /// Optional prompt/input content. Populated (via `AgentProgress::TurnContent`)
    /// **only** when `observability.agent_tracing.capture_content` is opted in —
    /// the [`super::collector::SpanCollector`] drops content at storage time otherwise, so with the
    /// default gate off this is always `None` and no exporter can serialize it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
    /// Optional model-reply/output content. Same storage-level gating as
    /// [`Self::input`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
}

impl TraceSpan {
    /// Duration in milliseconds, or `None` while the span is still open.
    #[cfg(test)]
    pub fn duration_ms(&self) -> Option<u64> {
        self.end_unix_ms
            .map(|end| end.saturating_sub(self.start_unix_ms))
    }
}
