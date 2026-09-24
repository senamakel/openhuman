//! Typed display items for the transcript projection RPC
//! (`threads.transcript_get`).
//!
//! These mirror the frontend's existing chat vocabulary (user/assistant
//! bubbles, reasoning drawer, tool timeline rows, sub-agent activity) so the
//! Phase C renderer can map them onto the same components. Serde is camelCase
//! on the wire — the frontend reads `displayContent`, `callId`, `requestId`,
//! etc.

use serde::Serialize;

/// Terminal state of a projected tool call. Mirrors the live timeline's
/// `ToolTimelineStatus` vocabulary (`running` / `success` / `error`) so the
/// settled projection and the live stream render identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Call issued but no result line has been paired yet.
    Running,
    /// A result line was paired to the call.
    Success,
    /// A result line the projection identified as a **failure**: the persisted
    /// tool line carried the additive `failure` flag (stamped at turn-loop
    /// persistence from the tool's `ToolResult::is_error` outcome). Paired with
    /// a [`ToolCallFailure`] payload on the item.
    Error,
}

/// Terminal state of a projected sub-agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    /// The run ended with a final answer.
    Completed,
    /// The spawning call failed, or reported the run incomplete.
    Failed,
    /// The run's last record is an interrupted partial answer.
    Interrupted,
    /// No terminal record yet (still running, or never settled).
    Running,
}

/// Failure payload attached to an errored [`DisplayItem::ToolCall`]. Minimal by
/// design: the persisted transcript only records that the call failed plus an
/// optional short reason. The frontend mapper expands this into its richer
/// `ToolFailureExplanation` shape (`class` / `category` / `causePlain` /
/// `nextAction`) for the `ToolFailureLines` renderer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallFailure {
    /// Short, single-line reason for the failure, when the writer captured one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One item in a projected transcript, in the frontend's display vocabulary.
///
/// `#[serde(tag = "kind")]` gives each variant a camelCase discriminator
/// (`userMessage`, `assistantMessage`, …) and every field is camelCase.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DisplayItem {
    /// A user prompt. `content` is the raw persisted content (may carry the
    /// injected `Current Date & Time:` scaffolding line); `displayContent` is
    /// the sanitized version to show, present only when it differs from raw.
    UserMessage {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// An assistant answer. `interim: true` marks a non-terminal tool-calling
    /// step within a multi-iteration turn (not the final answer bubble).
    AssistantMessage {
        content: String,
        #[serde(default, skip_serializing_if = "is_false")]
        interim: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        iteration: Option<u32>,
    },
    /// The model's reasoning/thinking that preceded an assistant message.
    /// `iteration` is the model call it belongs to — the same value as the
    /// message/tool calls that follow it — so a renderer groups it with the
    /// step it explains rather than the step before.
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        iteration: Option<u32>,
    },
    /// A tool invocation with its paired result, when available.
    ToolCall {
        call_id: String,
        name: String,
        /// The model call (1-based, within the turn) that issued this call.
        #[serde(skip_serializing_if = "Option::is_none")]
        iteration: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        status: ToolCallStatus,
        /// Present only when `status` is `Error` — the failure payload the
        /// frontend expands for the `ToolFailureLines` renderer.
        #[serde(skip_serializing_if = "Option::is_none")]
        failure: Option<ToolCallFailure>,
    },
    /// A delegated sub-agent run, with its own nested projected items.
    ///
    /// Placed in the item list directly after the tool call that spawned it
    /// when that call can be correlated (`call_id`), else at the end of the
    /// turn it was spawned in. Sub-agent transcripts are sibling files with no
    /// explicit back-link to the delegating tool call, so the turn is derived
    /// by matching the sub-agent's spawn timestamp (encoded in its file stem)
    /// against the parent turns' commit timestamps, and the call within that
    /// turn by the delegation target (see `subagents::attach`).
    ///
    /// `id` is unique per run: the spawn `task_id` when recorded, else the
    /// file-stem suffix — never the agent name, which repeats across runs.
    Subagent {
        id: String,
        /// Sub-agent definition id (e.g. `researcher`).
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        /// Spawn task id (`sub-…`), when the transcript recorded one.
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        /// The parent tool call that spawned this run, when correlated.
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        /// Terminal state of the run, derived from its own transcript and the
        /// spawning call's result.
        status: SubagentStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        items: Vec<DisplayItem>,
    },
    /// A turn boundary — emitted when the `request_id` changes between lines.
    TurnBoundary { request_id: String },
    /// A partial assistant answer captured when a turn was interrupted.
    InterruptedPartial {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
    },
    /// A context-compaction marker: the reduced set replaced everything before
    /// it. Counts describe what the record superseded/installed.
    Compaction {
        replaced_count: usize,
        kept_count: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        ts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

/// A projected transcript for one thread, before pagination. Chronological
/// (file) order; the RPC layer paginates newest-first.
#[derive(Debug, Clone)]
pub struct ProjectedTranscript {
    pub thread_id: String,
    /// All top-level display items in chronological order.
    pub items: Vec<DisplayItem>,
}
