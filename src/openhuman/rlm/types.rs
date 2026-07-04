//! Serde domain types for the `rlm` language-workflow tool.
//!
//! These cross the tool boundary (parsed from the `rlm` tool call, and rendered
//! back into its [`ToolResult`](crate::openhuman::tools::ToolResult)). The
//! runtime types that wire a session to openhuman's tools/models/subagents live
//! in [`super::bridge`] and [`super::sessions`]; the tinyagents-side limit and
//! result types live in that crate.

use serde::{Deserialize, Serialize};

/// A caller-supplied RLM session id. Continuing a prior `session_id` reuses that
/// session's persistent namespace (`let` bindings survive across cells); an
/// absent id starts a fresh session. Namespaces are additionally scoped by the
/// parent thread inside [`super::sessions`], so two chats never collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RlmSessionId(pub String);

impl RlmSessionId {
    /// The session id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RlmSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Per-call limit overrides a caller may pass in the `rlm` tool's `limits`
/// argument. Each is clamped in [`super::policy`] to a hard ceiling (never
/// unbounded); a `full`-tier caller may raise them, others are capped at the
/// conservative defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RlmLimitsOverride {
    /// Requested `max_tool_calls` for the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<usize>,
    /// Requested `max_agent_calls` for the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_agent_calls: Option<usize>,
    /// Requested `max_model_calls` for the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_model_calls: Option<usize>,
    /// Requested `max_concurrency` for batched calls (hard-capped at 8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<usize>,
}

/// A parsed `rlm` tool call — one cell to evaluate against a session.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RlmEvalRequest {
    /// The Rhai workflow cell to evaluate.
    pub script: String,
    /// Continue a prior session's namespace; `None` starts a fresh session.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Per-cell wall-clock timeout in seconds (clamped 1–3600).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Optional per-session limit overrides.
    #[serde(default)]
    pub limits: Option<RlmLimitsOverride>,
    /// Close (drop) the session after this cell.
    #[serde(default)]
    pub close_session: bool,
}

/// A summarized capability call a cell performed — kind, name, timing, and
/// success only. Never the raw arguments or payloads (those stay at `debug` log
/// level and on the live event stream), so a model-visible result cannot leak a
/// large or sensitive payload back into the context window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RlmCallSummary {
    /// `model` | `tool` | `agent` | `graph` | `emit`.
    pub kind: String,
    /// The capability or event name.
    pub name: String,
    /// Wall-clock time the call took, in milliseconds.
    pub elapsed_ms: u64,
    /// Whether the call was recorded (calls that errored abort the cell, so a
    /// recorded call is a completed one).
    pub ok: bool,
}

/// The remaining per-session budget after a cell, surfaced so the model can plan
/// how much more fan-out it can do before splitting work across sessions.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RlmLimitsRemaining {
    /// Cells left before `max_iterations`.
    pub cells: usize,
    /// `model_query` calls left.
    pub model_calls: usize,
    /// `tool_call` calls left.
    pub tool_calls: usize,
    /// `agent_query` calls left.
    pub agent_calls: usize,
}

/// The structured result of evaluating one RLM cell, rendered into the JSON
/// content of the tool result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RlmEvalResponse {
    /// The session the cell ran in (echoed so a fresh session's generated id is
    /// discoverable, and a later cell can continue it).
    pub session_id: String,
    /// Captured `print`/`debug` output.
    pub stdout: String,
    /// The cell's final expression value, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Names of persistent variables the cell created or changed.
    pub variables_changed: Vec<String>,
    /// Summarized capability calls the cell performed.
    pub calls: Vec<RlmCallSummary>,
    /// The final answer, if the cell called `answer(...)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_answer: Option<String>,
    /// Wall-clock time the cell took, in milliseconds.
    pub elapsed_ms: u64,
    /// Number of cells this session has evaluated so far.
    pub cells_used: usize,
    /// Remaining per-session budget.
    pub limits_remaining: RlmLimitsRemaining,
    /// Whether the session was closed after this cell.
    #[serde(default)]
    pub closed: bool,
}
