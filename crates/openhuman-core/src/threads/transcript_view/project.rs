//! Project raw session-transcript records into typed display items.
//!
//! Turns the append-only log's [`DisplayRecord`]s (message lines, compaction
//! markers, interrupted partials) into the frontend's chat vocabulary
//! ([`DisplayItem`]), sanitizing injected scaffolding as it goes. File
//! resolution lives in [`super::resolve`]; sub-agent trails are placed by
//! [`super::subagents`].

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use tinyagents_session::transcript::{self, CompactionMarker, DisplayMessage, DisplayRecord};
use tinytools_agent::dialect::{parse_replayed_results, ToolResultEntry};

use crate::agent::messages::TOOL_RESULT_FAILURES_METADATA_KEY;

use super::resolve;
use super::subagents;
use super::types::{DisplayItem, ProjectedTranscript, ToolCallFailure, ToolCallStatus};

const LOG_PREFIX: &str = "[threads][transcript]";

/// The scaffolding line injected onto every user message (see
/// `agent::prompts::current_datetime_line`). Stripped at projection so the UI
/// shows the user's actual words, not the per-turn time stamp.
const DATETIME_PREFIX: &str = "Current Date & Time:";

/// A legacy/alternate channel-context prefix. Kept for defensiveness; the
/// live injector currently only prepends [`DATETIME_PREFIX`].
const CHANNEL_CONTEXT_PREFIX: &str = "[Channel context]";

type NativeToolCall = (String, String, String);
type NativeToolEnvelope = (String, Vec<NativeToolCall>);

/// Resolve a thread's root transcript, discover its sub-agent siblings, and
/// project everything into display items. Returns `None` when the thread has
/// no root transcript yet (brand-new thread / first turn not persisted).
pub fn project_thread(workspace_dir: &Path, thread_id: &str) -> Option<ProjectedTranscript> {
    let (root_paths, sub_paths) = resolve_files(workspace_dir, thread_id)?;
    Some(project_from_files(thread_id, &root_paths, &sub_paths))
}

/// Resolve the on-disk file set backing a thread's transcript view: the root
/// generations in chain order plus every sub-agent sibling file. `None` when
/// the thread has no root transcript yet. Exposed so the cache can key on
/// these paths (and their mtimes/lengths) without re-projecting.
pub fn resolve_files(
    workspace_dir: &Path,
    thread_id: &str,
) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    resolve::resolve_files(workspace_dir, thread_id)
}

/// Project a thread from an already-resolved file set (root generations +
/// sub-agent siblings). Missing/unreadable files degrade to empty rather than
/// failing.
///
/// A root whose `_meta.parent_session_id` names the previous root is that
/// session's next compaction generation: it opens with the retained set
/// rewritten, so those rows are dropped (see [`resolve::drop_retained_rows`])
/// and a [`DisplayItem::Compaction`] marks the seam instead.
pub fn project_from_files(
    thread_id: &str,
    root_paths: &[PathBuf],
    sub_paths: &[PathBuf],
) -> ProjectedTranscript {
    log::debug!(
        "{LOG_PREFIX} projecting thread={thread_id} roots={} subagent_files={}",
        root_paths.len(),
        sub_paths.len()
    );

    let mut records: Vec<DisplayRecord> = Vec::new();
    let mut previous: Option<(Option<String>, Vec<DisplayRecord>)> = None;
    for root_path in root_paths {
        let display = match transcript::read_transcript_display(root_path) {
            Ok(display) => display,
            Err(err) => {
                log::warn!(
                    "{LOG_PREFIX} failed to read root transcript {}: {err}",
                    root_path.display()
                );
                continue;
            }
        };
        let successor_of_previous = matches!(
            (&previous, display.meta.parent_session_id.as_deref()),
            (Some((Some(prev_id), _)), Some(parent)) if prev_id == parent
        );
        if successor_of_previous {
            let predecessor = previous
                .as_ref()
                .map(|(_, prev_records)| resolve::generation_rows(prev_records))
                .unwrap_or_default();
            let (kept, retained) = resolve::drop_retained_rows(&display.records, predecessor);
            log::debug!(
                "{LOG_PREFIX} generation {} retained={} new_records={}",
                root_path.display(),
                retained.len(),
                kept.len()
            );
            let first_kept = kept.iter().find_map(|record| match record {
                DisplayRecord::Message(msg) => Some(msg),
                DisplayRecord::Compaction(_) => None,
            });
            let request_id = first_kept.and_then(|message| message.request_id.clone());
            records.push(DisplayRecord::Compaction(CompactionMarker {
                replacement: retained,
                ts: first_kept
                    .and_then(|message| message.ts.clone())
                    .or_else(|| Some(display.meta.created.clone()))
                    .filter(|ts| !ts.is_empty()),
                request_id,
            }));
            records.extend(kept);
        } else {
            records.extend(display.records.iter().cloned());
        }
        previous = Some((display.meta.session_id.clone(), display.records));
    }

    let mut items = project_records(&records);
    let top_level = items.len();
    subagents::attach(&mut items, sub_paths, &subagents::turn_segments(&records));
    log::debug!(
        "{LOG_PREFIX} projected thread={thread_id} top_level_items={top_level} subagents={}",
        items.len() - top_level
    );

    ProjectedTranscript {
        thread_id: thread_id.to_string(),
        items,
    }
}

/// Project one file's display records into display items, in file order.
///
/// - System lines are dropped (they carry the tool-policy preamble and other
///   scaffolding that must never render as a chat item).
/// - `reasoning_content` on an assistant line becomes a [`DisplayItem::Reasoning`]
///   preceding its message, carrying the same `iteration`.
/// - An assistant line's tool calls come from its native provider envelope.
///   Only a line that is *not* an envelope falls back to the calls recorded on
///   its usage (legacy text-dialect rows) — and never to a call id already
///   projected, which is how an aggregate list copied onto a final answer
///   would otherwise duplicate every call of the turn.
/// - Each call registers a pending [`DisplayItem::ToolCall`]; a later
///   `role:"tool"` line pairs to one by id, falling back to FIFO order.
/// - Interrupted partials and compaction markers pass through as their items.
/// - A [`DisplayItem::TurnBoundary`] is emitted whenever `request_id` changes.
pub fn project_records(records: &[DisplayRecord]) -> Vec<DisplayItem> {
    let mut projector = Projector::default();
    for record in records {
        match record {
            DisplayRecord::Message(msg) => {
                projector.turn_boundary(msg);
                projector.message(msg);
            }
            DisplayRecord::Compaction(marker) => {
                project_compaction(marker, &mut projector.items);
                // A compaction supersedes prior context; drop stale pending
                // pairings so a post-compaction result never binds to them.
                projector.pending.clear();
            }
        }
    }
    projector.items
}

#[derive(Default)]
struct Projector {
    items: Vec<DisplayItem>,
    /// Pending tool calls awaiting a result line: (call_id, index into `items`).
    pending: VecDeque<(String, usize)>,
    last_request_id: Option<String>,
    /// Every tool-call id already projected, so a row repeating calls issued
    /// earlier (the aggregate usage list) does not render them twice.
    seen_call_ids: HashSet<String>,
    /// The model-call ordinal of the last assistant row in the current turn —
    /// the fallback `iteration` for rows written without one.
    step: u32,
}

impl Projector {
    fn turn_boundary(&mut self, msg: &DisplayMessage) {
        let Some(rid) = msg.request_id.as_deref() else {
            return;
        };
        if self.last_request_id.as_deref() != Some(rid) {
            self.items.push(DisplayItem::TurnBoundary {
                request_id: rid.to_string(),
            });
            self.last_request_id = Some(rid.to_string());
            self.step = 0;
            self.seen_call_ids.clear();
        }
    }

    fn message(&mut self, msg: &DisplayMessage) {
        // Interrupted partial: display-only, carries its own thinking.
        if msg.interrupted {
            self.items.push(DisplayItem::InterruptedPartial {
                text: msg.message.content.clone(),
                thinking: msg.reasoning_content.clone(),
            });
            return;
        }

        match msg.message.role.as_str() {
            "system" => {
                // Scaffolding (tool-policy preamble, etc.) — never a display item.
                log::debug!("{LOG_PREFIX} sanitize: dropped system line from projection");
            }
            "user" => {
                // A text dialect folds a round's results into one user turn;
                // it is tool output, never the user's words.
                if let Some(results) = parse_replayed_results(&msg.message.content) {
                    project_text_tool_results(msg, results, &mut self.items, &mut self.pending);
                    return;
                }
                // A legacy turn without request ids still restarts the step
                // count at its prompt.
                self.step = 0;
                self.seen_call_ids.clear();
                let raw = msg.message.content.clone();
                let sanitized = sanitize_user_content(&raw);
                if sanitized.is_some() {
                    log::debug!(
                        "{LOG_PREFIX} sanitize: stripped injected prefix from user message"
                    );
                }
                self.items.push(DisplayItem::UserMessage {
                    content: raw,
                    display_content: sanitized,
                    request_id: msg.request_id.clone(),
                });
            }
            "assistant" => self.assistant(msg),
            "tool" => self.tool_result(msg),
            other => {
                log::debug!("{LOG_PREFIX} projecting unknown role {other:?} as assistant message");
                self.items.push(DisplayItem::AssistantMessage {
                    content: msg.message.content.clone(),
                    interim: false,
                    request_id: msg.request_id.clone(),
                    model: msg.turn_usage.as_ref().map(|tu| tu.model.clone()),
                    iteration: msg.iteration,
                });
            }
        }
    }

    fn assistant(&mut self, msg: &DisplayMessage) {
        // Rows the writer stamped keep their own number; earlier writers only
        // stamped the final row, so unstamped rows count up within the turn.
        let iteration = msg.iteration.unwrap_or(self.step + 1);
        self.step = iteration;

        // Reasoning precedes the message it belongs to.
        if let Some(reasoning) = msg.reasoning_content.as_deref() {
            if !reasoning.trim().is_empty() {
                self.items.push(DisplayItem::Reasoning {
                    text: reasoning.to_string(),
                    iteration: Some(iteration),
                });
            }
        }

        let native_envelope = parse_native_tool_envelope(&msg.message.content);
        let tool_calls: Vec<NativeToolCall> = match &native_envelope {
            Some((_, calls)) => calls.clone(),
            None => {
                let recorded: Vec<NativeToolCall> = msg
                    .turn_usage
                    .as_ref()
                    .map(|tu| {
                        tu.tool_calls
                            .iter()
                            .map(|call| {
                                (call.id.clone(), call.name.clone(), call.arguments.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let total = recorded.len();
                let fresh: Vec<NativeToolCall> = recorded
                    .into_iter()
                    .filter(|(id, _, _)| id.is_empty() || !self.seen_call_ids.contains(id))
                    .collect();
                if fresh.len() < total {
                    log::debug!(
                        "{LOG_PREFIX} dropped {} already-projected tool call(s) repeated on an assistant row iteration={iteration}",
                        total - fresh.len()
                    );
                }
                fresh
            }
        };
        let interim = !tool_calls.is_empty();

        // Native tool-call turns are persisted as their provider envelope so they
        // can be replayed byte-faithfully. The display projection needs only the
        // envelope's visible `content`; rendering/sanitizing the whole JSON object
        // makes the narration disappear (and risks showing raw tool JSON).
        let visible_content = native_envelope
            .map(|(content, _)| content)
            .unwrap_or_else(|| msg.message.content.clone());

        // The assistant's prose (if any) shows before its tool calls.
        if !visible_content.trim().is_empty() {
            self.items.push(DisplayItem::AssistantMessage {
                content: visible_content,
                interim,
                request_id: msg.request_id.clone(),
                model: msg.turn_usage.as_ref().map(|tu| tu.model.clone()),
                iteration: Some(iteration),
            });
        }

        for (call_id, name, arguments) in tool_calls {
            let args = parse_tool_args(&arguments);
            // Repair legacy rows which put aggregate calls on the final
            // answer after their result had already projected as an orphan.
            if let Some(DisplayItem::ToolCall {
                name: settled_name,
                args: settled_args,
                ..
            }) = settled_orphan_mut(&mut self.items, &call_id)
            {
                log::debug!("{LOG_PREFIX} call {call_id} recorded after its result — merged");
                *settled_name = name;
                *settled_args = args;
                continue;
            }
            if !call_id.is_empty() {
                self.seen_call_ids.insert(call_id.clone());
            }
            self.items.push(DisplayItem::ToolCall {
                call_id: call_id.clone(),
                name,
                iteration: Some(iteration),
                args,
                result: None,
                status: ToolCallStatus::Running,
                failure: None,
            });
            self.pending.push_back((call_id, self.items.len() - 1));
        }
    }

    fn tool_result(&mut self, msg: &DisplayMessage) {
        let (result, wrapped_id) = unwrap_tool_result(&msg.message.content);
        // A failed tool line (`ToolResult::is_error`, stamped at persistence)
        // pairs to an error row with a failure payload instead of a false
        // success.
        let (status, failure) = if msg.failure {
            (
                ToolCallStatus::Error,
                Some(ToolCallFailure {
                    detail: msg.failure_detail.clone(),
                }),
            )
        } else {
            (ToolCallStatus::Success, None)
        };
        let call_id = msg.message.id.clone().or(wrapped_id);
        // Pair by explicit call id first, else FIFO.
        let idx = call_id
            .as_deref()
            .and_then(|id| take_pending_by_id(&mut self.pending, id))
            .or_else(|| self.pending.pop_front().map(|(_, idx)| idx));

        if let Some(idx) = idx {
            if let Some(DisplayItem::ToolCall {
                result: slot,
                status: status_slot,
                failure: failure_slot,
                ..
            }) = self.items.get_mut(idx)
            {
                *slot = Some(result);
                *status_slot = status;
                *failure_slot = failure;
                return;
            }
        }

        // Orphan result (no matching assistant tool_call recorded) — surface it
        // as a best-effort completed tool row so the output is not lost.
        log::debug!("{LOG_PREFIX} tool result with no pending call — emitting orphan tool row");
        self.items.push(DisplayItem::ToolCall {
            call_id: call_id.unwrap_or_default(),
            name: "tool".to_string(),
            iteration: None,
            args: None,
            result: Some(result),
            status,
            failure,
        });
    }
}

/// Decode the native provider replay envelope embedded in `ChatMessage.content`.
/// Returns visible assistant prose plus `(id, name, arguments)` calls.
pub(super) fn parse_native_tool_envelope(raw: &str) -> Option<NativeToolEnvelope> {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let object = value.as_object()?;
    let calls = object.get("tool_calls")?.as_array()?;
    let content = match object.get("content") {
        Some(serde_json::Value::String(content)) => content.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        _ => return None,
    };
    let calls = calls
        .iter()
        .filter_map(|call| {
            let call = call.as_object()?;
            let id = call.get("id")?.as_str()?.to_string();
            let name = call.get("name")?.as_str()?.to_string();
            let arguments = match call.get("arguments") {
                Some(serde_json::Value::String(arguments)) => arguments.clone(),
                Some(arguments) => arguments.to_string(),
                None => String::new(),
            };
            Some((id, name, arguments))
        })
        .collect();
    Some((content, calls))
}

/// Keys a native tool-result replay envelope may carry besides `content`.
const TOOL_RESULT_ENVELOPE_KEYS: &[&str] = &["content", "tool_call_id", "tool_name", "name"];

/// Unwrap a native tool-result replay envelope
/// (`{"tool_call_id":…,"content":…}`) to the tool's own output plus the call
/// id it names. Anything else — including a tool whose genuine output happens
/// to be JSON with other keys — is returned verbatim.
fn unwrap_tool_result(raw: &str) -> (String, Option<String>) {
    let Ok(serde_json::Value::Object(object)) = serde_json::from_str::<serde_json::Value>(raw)
    else {
        return (raw.to_string(), None);
    };
    let (Some(content), Some(call_id)) = (
        object.get("content").and_then(serde_json::Value::as_str),
        object
            .get("tool_call_id")
            .and_then(serde_json::Value::as_str),
    ) else {
        return (raw.to_string(), None);
    };
    if !object
        .keys()
        .all(|key| TOOL_RESULT_ENVELOPE_KEYS.contains(&key.as_str()))
    {
        return (raw.to_string(), None);
    }
    (
        content.to_string(),
        Some(call_id.to_string()).filter(|id| !id.is_empty()),
    )
}

/// Pair each result of a text-dialect `[Tool results]` row with its pending
/// call. Failure status comes from the ids the session codec recorded on the
/// row ([`TOOL_RESULT_FAILURES_METADATA_KEY`]); a result with no pending call
/// surfaces as an orphan row, as for a native `tool` line.
fn project_text_tool_results(
    msg: &DisplayMessage,
    results: Vec<ToolResultEntry>,
    items: &mut Vec<DisplayItem>,
    pending: &mut VecDeque<(String, usize)>,
) {
    let failed: Vec<&str> = msg
        .message
        .extra_metadata
        .as_ref()
        .and_then(|meta| meta.get(TOOL_RESULT_FAILURES_METADATA_KEY))
        .and_then(serde_json::Value::as_array)
        .map(|ids| ids.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    log::debug!(
        "{LOG_PREFIX} text-dialect results row results={} failed={} pending={}",
        results.len(),
        failed.len(),
        pending.len()
    );
    for result in results {
        let (status, failure) = if failed.contains(&result.tool_call_id.as_str()) {
            (
                ToolCallStatus::Error,
                Some(ToolCallFailure { detail: None }),
            )
        } else {
            (ToolCallStatus::Success, None)
        };
        if let Some(idx) = take_pending_by_id(pending, &result.tool_call_id) {
            if let Some(DisplayItem::ToolCall {
                result: slot,
                status: status_slot,
                failure: failure_slot,
                ..
            }) = items.get_mut(idx)
            {
                *slot = Some(result.content);
                *status_slot = status;
                *failure_slot = failure;
                continue;
            }
        }
        items.push(DisplayItem::ToolCall {
            call_id: result.tool_call_id,
            name: "tool".to_string(),
            args: None,
            result: Some(result.content),
            status,
            failure,
        });
    }
}

/// The already-settled orphan row for `call_id` in the current turn — a result
/// that projected before any call named it.
fn settled_orphan_mut<'a>(
    items: &'a mut [DisplayItem],
    call_id: &str,
) -> Option<&'a mut DisplayItem> {
    let turn_start = items
        .iter()
        .rposition(|item| matches!(item, DisplayItem::TurnBoundary { .. }))
        .map_or(0, |idx| idx + 1);
    items[turn_start..].iter_mut().find(|item| {
        matches!(
            item,
            DisplayItem::ToolCall { call_id: id, name, result: Some(_), .. }
                if id == call_id && name == "tool"
        )
    })
}

/// Remove and return the pending entry whose call id matches `id`, if any.
fn take_pending_by_id(pending: &mut VecDeque<(String, usize)>, id: &str) -> Option<usize> {
    let pos = pending.iter().position(|(cid, _)| cid == id)?;
    pending.remove(pos).map(|(_, idx)| idx)
}

fn project_compaction(marker: &CompactionMarker, items: &mut Vec<DisplayItem>) {
    items.push(DisplayItem::Compaction {
        replaced_count: 0,
        kept_count: marker.replacement.len(),
        ts: marker.ts.clone(),
        request_id: marker.request_id.clone(),
    });
}

/// Parse a tool call's raw argument string into JSON when possible; a
/// non-JSON string is wrapped so the frontend still receives structured args.
fn parse_tool_args(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => Some(value),
        Err(_) => Some(serde_json::Value::String(trimmed.to_string())),
    }
}

/// Strip the injected scaffolding prefix from a user message, returning the
/// sanitized body only when a prefix was actually present (so the caller can
/// tag rather than mutate — the raw `content` is preserved alongside).
fn sanitize_user_content(content: &str) -> Option<String> {
    let trimmed_start = content.trim_start();
    if trimmed_start.starts_with(DATETIME_PREFIX)
        || trimmed_start.starts_with(CHANNEL_CONTEXT_PREFIX)
    {
        // The injector prepends the scaffolding line followed by a blank line,
        // then the user's actual text. Strip the first paragraph.
        if let Some(idx) = content.find("\n\n") {
            let body = content[idx + 2..].to_string();
            return Some(body);
        }
        // No body after the prefix — the whole message was scaffolding.
        return Some(String::new());
    }
    None
}
