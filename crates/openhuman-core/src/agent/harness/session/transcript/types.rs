//! Public transcript domain types: per-message usage, the `_meta` header,
//! the model-context and display projections, and thread usage summaries.

use crate::agent::messages::ChatMessage;
use crate::inference::provider::ToolCall;
use serde::{Deserialize, Serialize};

/// Per-message usage figures attributed to the last assistant turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUsage {
    pub input: u64,
    pub output: u64,
    pub cached_input: u64,
    #[serde(default)]
    pub context_window: u64,
    pub cost_usd: f64,
}

/// Usage + provenance for one provider response, attached to the last
/// assistant message in a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnUsage {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    pub usage: MessageUsage,
    /// RFC-3339 timestamp of the response.
    #[serde(default)]
    pub ts: String,
    /// Raw reasoning/thinking content returned by thinking models. This is
    /// persisted as metadata so the later transcript view can show the model's
    /// thoughts without depending on the live stream still being open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Native tool calls emitted in this provider response, if any. Text-mode
    /// calls remain present in `content` as the raw markup the model emitted.
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// One-based engine iteration for this provider response.
    #[serde(default)]
    pub iteration: u32,
}

/// Schema version stamped on the `_meta` header line. Bumped when the JSONL
/// record shape changes in a way future readers may need to branch on. `0`
/// (absent) denotes pre-append-only files written before this field existed.
pub const TRANSCRIPT_SCHEMA_VERSION: u32 = 1;

/// Metadata header for a session transcript file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMeta {
    pub agent_name: String,
    /// Canonical registry id for the agent that produced this transcript.
    /// `agent_name` may be per-thread renamed for file names; this remains the
    /// stable archetype id when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Coarse runtime kind (`root`, `subagent`, `extractor`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub dispatcher: String,
    /// Provider label used for the most recent recorded response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Model id used for the most recent recorded response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub created: String,
    pub updated: String,
    pub turn_count: usize,
    /// Cumulative input tokens across all provider calls this session.
    pub input_tokens: u64,
    /// Cumulative output tokens across all provider calls this session.
    pub output_tokens: u64,
    /// Cumulative input tokens served from the KV cache.
    pub cached_input_tokens: u64,
    /// Cumulative amount charged in USD.
    pub charged_amount_usd: f64,
    /// Backend-side LLM thread identifier (the `thread_id` forwarded on
    /// `/openai/v1/chat/completions` so the OpenHuman backend can group
    /// `InferenceLog` entries and align KV-cache keys with the same logical
    /// chat thread the user sees in the UI). `None` for runs that don't
    /// originate from a thread-scoped channel (e.g. CLI-only sessions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Sub-agent task id, when this transcript belongs to a spawned worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// A parsed session transcript: metadata + exact message array.
#[derive(Debug, Clone)]
pub struct SessionTranscript {
    pub meta: TranscriptMeta,
    pub messages: Vec<ChatMessage>,
}

// ── Display read types ───────────────────────────────────────────────

/// One message in a display projection, carrying the turn-boundary + partial
/// flags the model-context [`SessionTranscript`] discards.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub message: ChatMessage,
    /// `true` when this is an interrupted partial answer (display only).
    pub interrupted: bool,
    /// Turn boundary marker (`request_id`), when stamped.
    pub request_id: Option<String>,
    pub iteration: Option<u32>,
    pub ts: Option<String>,
    /// Usage/provenance for assistant messages that carried it.
    pub turn_usage: Option<TurnUsage>,
    /// Raw reasoning/thinking captured for this line, when present. Mirrors the
    /// line's `reasoning_content` directly so it survives even on lines without
    /// full turn-usage provenance (e.g. an interrupted partial, which carries no
    /// provider/model/usage). Prefer this over digging into [`Self::turn_usage`]
    /// for display: it is populated from `turn_usage.reasoning_content` too.
    pub reasoning_content: Option<String>,
    /// `true` when this is a **failed** tool-result line (`ToolResult::is_error`
    /// at execution time). The display projection renders an error tool row
    /// instead of success. Always `false` for non-tool lines and legacy files.
    pub failure: bool,
    /// Optional short reason for a failed tool call (present only with
    /// `failure: true`).
    pub failure_detail: Option<String>,
}

/// A compaction marker in a display projection.
#[derive(Debug, Clone)]
pub struct CompactionMarker {
    /// The reduced message set this compaction installed as the new context.
    pub replacement: Vec<DisplayMessage>,
    pub ts: Option<String>,
    pub request_id: Option<String>,
}

/// One record in a display projection, in file order.
#[derive(Debug, Clone)]
pub enum DisplayRecord {
    Message(DisplayMessage),
    Compaction(CompactionMarker),
}

/// A display projection of a transcript: **all** records, including
/// pre-compaction history, compaction markers, and interrupted partials.
#[derive(Debug, Clone)]
pub struct DisplaySessionTranscript {
    pub meta: TranscriptMeta,
    pub records: Vec<DisplayRecord>,
}

/// Aggregated token/cost usage for a chat thread, summed across **all** of the
/// thread's root session transcripts (a thread reopened across days/restarts
/// produces several files). `last_turn_*`, `model`, and `updated` come from the
/// newest transcript so the UI can render a context-window gauge for the most
/// recent turn. Returns `None` when no transcript exists yet (a brand-new
/// thread with no completed turns).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadUsageSummary {
    /// Orchestrator (parent) token totals — the root transcript(s) only. Root
    /// transcripts never include sub-agent calls (those go to a separate
    /// observer + their own `__` transcript files); see [`Self::subagents`].
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    pub cost_usd: f64,
    pub turn_count: usize,
    /// Input/output tokens of the most recent assistant turn (context gauge).
    pub last_turn_input_tokens: u64,
    pub last_turn_output_tokens: u64,
    /// Model that served the most recent turn, if recorded.
    pub model: Option<String>,
    /// RFC-3339 `updated` of the newest transcript.
    pub updated: String,
    /// Per-archetype sub-agent spend, reconstructed from the thread's `__`
    /// sub-agent transcripts (grouped by `agent_name`).
    pub subagents: Vec<SubagentArchetypeUsage>,
}

/// One sub-agent archetype's summed spend within a thread (e.g. all `coder`
/// runs). `model` is the model that served one of its runs, used to price it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubagentArchetypeUsage {
    pub agent_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
    /// How many sub-agent runs of this archetype contributed.
    pub runs: usize,
    pub model: Option<String>,
}
