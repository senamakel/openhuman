//! JSONL line shapes (`_meta`, message, and compaction records) and the
//! conversions between them and the public [`ChatMessage`] /
//! [`TranscriptMeta`] / [`DisplayMessage`] types.

use super::metadata::{attach_turn_usage_metadata, take_tool_failure};
use super::types::{
    DisplayMessage, MessageUsage, TranscriptMeta, TurnUsage, TRANSCRIPT_SCHEMA_VERSION,
};
use crate::agent::harness::session::transcript::metadata::turn_usage_from_metadata;
use crate::agent::messages::ChatMessage;
use crate::inference::provider::ToolCall;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Discriminator value for a compaction record's `kind` field.
pub(super) const COMPACTION_KIND: &str = "compaction";

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

// ── Internal JSONL types ─────────────────────────────────────────────

/// The `_meta` line serialisation shape.
#[derive(Serialize, Deserialize)]
pub(super) struct MetaLine {
    #[serde(rename = "_meta")]
    pub(super) meta: MetaPayload,
}

#[derive(Serialize, Deserialize)]
pub(super) struct MetaPayload {
    /// Schema version of the transcript record format (see
    /// [`TRANSCRIPT_SCHEMA_VERSION`]). Absent (deserialises to `0`) on files
    /// written before the append-only migration.
    #[serde(default)]
    pub(super) version: u32,
    pub(super) agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) agent_type: Option<String>,
    pub(super) dispatcher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model: Option<String>,
    pub(super) created: String,
    pub(super) updated: String,
    pub(super) turn_count: usize,
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) cached_input_tokens: u64,
    pub(super) charged_amount_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) task_id: Option<String>,
}

/// One message line in the JSONL — only `role` and `content` are required.
/// All other fields are optional; unknown fields are flattened to preserve
/// forward-compatibility.
#[derive(Serialize, Deserialize)]
pub(super) struct MessageLine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<String>,
    pub(super) role: String,
    pub(super) content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) extra_metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage: Option<MessageUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) iteration: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ts: Option<String>,
    /// Turn boundary marker: the web-chat `request_id` this message belongs to,
    /// when available. Stamped on every line of a turn so the display projection
    /// can group a turn's messages. Absent for CLI / non-request-scoped runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) request_id: Option<String>,
    /// `true` when this line is a *partial* assistant answer captured because
    /// the turn was interrupted/cancelled mid-stream. Present for **display
    /// only** — the model-context reader skips these so a resumed context never
    /// carries a truncated answer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) interrupted: bool,
    /// `true` when this tool-result line's tool call **failed**
    /// (`ToolResult::is_error`). Additive + optional: legacy lines and every
    /// non-tool line omit it and default to success. Lifted from the tool
    /// message's failure metadata by [`build_message_line`]; consumed by the
    /// display projection to render an error tool row instead of success.
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) failure: bool,
    /// Optional short, single-line reason for a failed tool call (the head of
    /// the error output). Present only alongside `failure: true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) failure_detail: Option<String>,
    /// Absorb any unknown fields so forward-compat reads don't error.
    #[serde(flatten)]
    pub(super) _extra: HashMap<String, serde_json::Value>,
}

/// A compaction record: `{"kind":"compaction","replacement":[…]}`.
///
/// Appended when the harness reduces context (post-compaction / trim) so the
/// model-context reader can reconstruct the reduced set without the file being
/// destructively rewritten. `replacement` is the **full** logical message set
/// that supersedes everything before it — an explicit replacement list
/// (mirroring Codex's `Compacted { replacement_history }`) rather than
/// surviving-message ids, because our writer already holds the reduced
/// `messages` slice on each persist call and message ids are optional, so an
/// id-reference scheme would be less robust for no gain.
#[derive(Serialize, Deserialize)]
pub(super) struct CompactionLine {
    pub(super) kind: String,
    pub(super) replacement: Vec<MessageLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) request_id: Option<String>,
    #[serde(flatten)]
    pub(super) _extra: HashMap<String, serde_json::Value>,
}

/// Build the serialised `_meta` header line for `meta`, stamping the current
/// [`TRANSCRIPT_SCHEMA_VERSION`].
fn meta_payload_from(meta: &TranscriptMeta) -> MetaPayload {
    MetaPayload {
        version: TRANSCRIPT_SCHEMA_VERSION,
        agent: meta.agent_name.clone(),
        agent_id: meta.agent_id.clone(),
        agent_type: meta.agent_type.clone(),
        dispatcher: meta.dispatcher.clone(),
        provider: meta.provider.clone(),
        model: meta.model.clone(),
        created: meta.created.clone(),
        updated: meta.updated.clone(),
        turn_count: meta.turn_count,
        input_tokens: meta.input_tokens,
        output_tokens: meta.output_tokens,
        cached_input_tokens: meta.cached_input_tokens,
        charged_amount_usd: meta.charged_amount_usd,
        thread_id: meta.thread_id.clone(),
        task_id: meta.task_id.clone(),
    }
}

pub(super) fn meta_line_json(meta: &TranscriptMeta) -> Result<String> {
    let meta_line = MetaLine {
        meta: meta_payload_from(meta),
    };
    serde_json::to_string(&meta_line).context("serialise transcript meta header")
}

/// Build a [`MessageLine`] for `msg`, folding in `turn_usage` (assistant rows)
/// and stamping the `request_id` turn boundary when supplied.
pub(super) fn build_message_line(
    msg: &ChatMessage,
    turn_usage: Option<&TurnUsage>,
    request_id: Option<&str>,
    interrupted: bool,
) -> MessageLine {
    let assistant_usage = if msg.role == "assistant" {
        turn_usage
    } else {
        None
    };
    // Lift any tool-failure marker off a cloned `extra_metadata` onto the
    // additive top-level `failure` / `failure_detail` line fields, stripping it
    // so it is not persisted twice.
    let mut extra_metadata = msg.extra_metadata.clone();
    let (failure, failure_detail) = match take_tool_failure(&mut extra_metadata) {
        Some((failed, detail)) => (failed, detail),
        None => (false, None),
    };
    let message_reasoning = (msg.role == "assistant")
        .then(|| {
            extra_metadata
                .as_ref()
                .and_then(|meta| meta.get(crate::agent::message_convert::REASONING_EXT_KEY))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .flatten();
    let native_envelope = (msg.role == "assistant")
        .then(|| serde_json::from_str::<serde_json::Value>(&msg.content).ok())
        .flatten();
    let envelope_reasoning = native_envelope
        .as_ref()
        .and_then(|value| value.get("reasoning_content"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let envelope_tool_calls = native_envelope
        .as_ref()
        .and_then(|value| value.get("tool_calls"))
        .and_then(|value| serde_json::from_value::<Vec<ToolCall>>(value.clone()).ok())
        .filter(|calls| !calls.is_empty());
    MessageLine {
        id: msg.id.clone(),
        role: msg.role.clone(),
        content: msg.content.clone(),
        extra_metadata,
        provider: assistant_usage.map(|tu| tu.provider.clone()),
        model: assistant_usage.map(|tu| tu.model.clone()),
        usage: assistant_usage.map(|tu| tu.usage.clone()),
        reasoning_content: message_reasoning
            .or(envelope_reasoning)
            .or_else(|| assistant_usage.and_then(|tu| tu.reasoning_content.clone())),
        tool_calls: envelope_tool_calls.or_else(|| {
            assistant_usage.and_then(|tu| {
                if tu.tool_calls.is_empty() {
                    None
                } else {
                    Some(tu.tool_calls.clone())
                }
            })
        }),
        iteration: assistant_usage.map(|tu| tu.iteration),
        ts: assistant_usage.map(|tu| tu.ts.clone()),
        request_id: request_id.map(str::to_string),
        interrupted,
        failure,
        failure_detail,
        _extra: HashMap::new(),
    }
}

/// Serialise `messages` into JSONL message lines, attributing
/// `last_assistant_turn_usage` (or per-message embedded usage) to the last
/// assistant row and stamping `request_id` on every line.
pub(super) fn serialise_message_lines(
    messages: &[ChatMessage],
    last_assistant_turn_usage: Option<&TurnUsage>,
    request_id: Option<&str>,
    buf: &mut String,
) -> Result<()> {
    let last_assistant_idx = messages.iter().rposition(|m| m.role == "assistant");
    for (i, msg) in messages.iter().enumerate() {
        let turn_usage = if Some(i) == last_assistant_idx {
            last_assistant_turn_usage
                .cloned()
                .or_else(|| turn_usage_from_metadata(msg))
        } else {
            turn_usage_from_metadata(msg)
        };
        let line = build_message_line(msg, turn_usage.as_ref(), request_id, false);
        let line_json =
            serde_json::to_string(&line).with_context(|| format!("serialise message line {i}"))?;
        buf.push_str(&line_json);
        buf.push('\n');
    }
    Ok(())
}

/// Convert a parsed `MetaPayload` into the public [`TranscriptMeta`].
pub(super) fn meta_from_payload(mp: MetaPayload) -> TranscriptMeta {
    TranscriptMeta {
        agent_name: mp.agent,
        agent_id: mp.agent_id,
        agent_type: mp.agent_type,
        dispatcher: mp.dispatcher,
        provider: mp.provider,
        model: mp.model,
        created: mp.created,
        updated: mp.updated,
        turn_count: mp.turn_count,
        input_tokens: mp.input_tokens,
        output_tokens: mp.output_tokens,
        cached_input_tokens: mp.cached_input_tokens,
        charged_amount_usd: mp.charged_amount_usd,
        thread_id: mp.thread_id,
        task_id: mp.task_id,
    }
}

/// Recover the [`TurnUsage`] a message line carried (assistant rows only).
fn turn_usage_from_line(ml: &MessageLine) -> Option<TurnUsage> {
    match (
        ml.provider.clone(),
        ml.model.clone(),
        ml.usage.clone(),
        ml.ts.clone(),
    ) {
        (Some(provider), Some(model), Some(usage), Some(ts)) if ml.role == "assistant" => {
            Some(TurnUsage {
                provider,
                model,
                usage,
                ts,
                reasoning_content: ml.reasoning_content.clone(),
                tool_calls: ml.tool_calls.clone().unwrap_or_default(),
                iteration: ml.iteration.unwrap_or_default(),
            })
        }
        _ => None,
    }
}

/// Reconstruct a [`ChatMessage`] from a message line, re-attaching turn-usage
/// metadata so the round-trip is lossless for the model-context path.
pub(super) fn message_from_line(ml: MessageLine) -> ChatMessage {
    let turn_usage = turn_usage_from_line(&ml);
    let mut message = ChatMessage {
        id: ml.id,
        role: ml.role,
        content: ml.content,
        extra_metadata: ml.extra_metadata,
        cache_breakpoints: Vec::new(),
    };
    if let Some(turn_usage) = turn_usage.as_ref() {
        attach_turn_usage_metadata(&mut message, turn_usage);
    }
    message
}

/// Classification of one non-empty JSONL line.
pub(super) enum LineKind {
    Meta(MetaLine),
    Compaction(CompactionLine),
    Message(MessageLine),
}

/// Classify a raw line: a `_meta` header/update, a `compaction` record, or a
/// message line. Returns `Err` only when the line is malformed for its
/// apparent kind; the caller decides whether that is fatal (first line) or a
/// skippable warning (later lines).
pub(super) fn classify_line(line: &str) -> Result<LineKind, serde_json::Error> {
    // Cheap structural peek. Unknown/other shapes fall through to MessageLine,
    // whose required `role`/`content` gate rejects genuinely foreign lines.
    let value: serde_json::Value = serde_json::from_str(line)?;
    if value.get("_meta").is_some() {
        return serde_json::from_str::<MetaLine>(line).map(LineKind::Meta);
    }
    if value.get("kind").and_then(|k| k.as_str()) == Some(COMPACTION_KIND) {
        return serde_json::from_str::<CompactionLine>(line).map(LineKind::Compaction);
    }
    serde_json::from_str::<MessageLine>(line).map(LineKind::Message)
}

// ── Display read ──────────────────────────────────────────────────────

/// Reconstruct a [`DisplayMessage`] from a message line, preserving the
/// turn-boundary + partial flags the model-context path discards.
pub(super) fn display_message_from_line(ml: MessageLine) -> DisplayMessage {
    let turn_usage = turn_usage_from_line(&ml);
    let reasoning_content = ml.reasoning_content.clone().or_else(|| {
        turn_usage
            .as_ref()
            .and_then(|tu| tu.reasoning_content.clone())
    });
    DisplayMessage {
        interrupted: ml.interrupted,
        request_id: ml.request_id.clone(),
        iteration: ml.iteration,
        ts: ml.ts.clone(),
        turn_usage,
        reasoning_content,
        failure: ml.failure,
        failure_detail: ml.failure_detail.clone(),
        message: ChatMessage {
            id: ml.id,
            role: ml.role,
            content: ml.content,
            extra_metadata: ml.extra_metadata,
            cache_breakpoints: Vec::new(),
        },
    }
}
