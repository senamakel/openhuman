//! Pure, terminal-free transcript reducer for the tabbed terminal UI's Chat tab.
//!
//! [`TranscriptState`] is a plain data structure with **no ratatui / crossterm /
//! IO dependencies** — the renderer ([`super::render`]) reads it and the event
//! loop ([`super::app`]) mutates it, but the state transitions themselves live
//! here so they can be unit-tested without a terminal.
//!
//! The single entry point is [`TranscriptState::apply_event`], which folds a
//! [`WebChannelEvent`] (the same struct the desktop app receives over Socket.IO)
//! into the transcript. Events for a different `client_id` are ignored, so a
//! process-wide broadcast bus can be drained safely.

use openhuman_core::core::socketio::WebChannelEvent;

/// The kind of a transcript entry — drives colour / prefix in the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A message the local user sent.
    User,
    /// The assistant's streamed / final reply text.
    Assistant,
    /// The assistant's "thinking" (reasoning) stream — rendered dimmed.
    Thinking,
    /// A tool-call / tool-result status line.
    Tool,
    /// A terminal error (`chat_error`).
    Error,
    /// A local status/system note (never produced by `apply_event`).
    System,
}

/// One line-group in the transcript. `text` accumulates across streaming deltas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
}

impl Entry {
    fn new(kind: EntryKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// Accumulated transcript + streaming status for one chat client stream.
#[derive(Debug, Clone)]
pub struct TranscriptState {
    /// Our stream identity. Events whose `client_id` differs are ignored.
    client_id: String,
    /// Active conversation. Events from another thread must not leak into it.
    thread_id: String,
    /// The rendered transcript, oldest first.
    entries: Vec<Entry>,
    /// True while a turn is in flight (between send and `chat_done`/`chat_error`).
    streaming: bool,
    /// Index into `entries` of the assistant entry currently accumulating text
    /// deltas for the in-flight turn, if any.
    cur_assistant: Option<usize>,
    /// Index into `entries` of the thinking entry currently accumulating
    /// thinking deltas for the in-flight turn, if any.
    cur_thinking: Option<usize>,
}

impl TranscriptState {
    /// Create an empty transcript bound to `client_id`.
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            thread_id: String::new(),
            entries: Vec::new(),
            streaming: false,
            cur_assistant: None,
            cur_thinking: None,
        }
    }

    /// The transcript entries, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Whether a turn is currently streaming.
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Our client stream id.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn set_thread(&mut self, thread_id: impl Into<String>) {
        self.thread_id = thread_id.into();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.finish_turn();
    }

    pub fn last_assistant(&self) -> Option<&str> {
        if self.streaming {
            return None;
        }
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.kind == EntryKind::Assistant)
            .map(|entry| entry.text.as_str())
    }

    pub fn export_markdown(&self) -> String {
        let mut out = String::from("# OpenHuman transcript\n\n");
        for entry in &self.entries {
            let title = match entry.kind {
                EntryKind::User => "You",
                EntryKind::Assistant => "OpenHuman",
                EntryKind::Thinking => "Reasoning",
                EntryKind::Tool => "Tool",
                EntryKind::Error => "Error",
                EntryKind::System => "System",
            };
            out.push_str("## ");
            out.push_str(title);
            out.push_str("\n\n");
            out.push_str(&entry.text);
            out.push_str("\n\n");
        }
        out
    }

    /// Replace the view with a newest-first `threads.transcript_get` page.
    pub fn load_transcript(&mut self, value: &serde_json::Value) {
        self.entries.clear();
        let page = super::cockpit::unwrap_rpc(value);
        let Some(items) = page.get("items").and_then(serde_json::Value::as_array) else {
            return;
        };
        for item in items.iter().rev() {
            self.push_projected_item(item);
        }
        self.finish_turn();
    }

    /// Record a locally-sent user message and begin a new turn.
    ///
    /// Resets the streaming cursors so the next `text_delta` / `thinking_delta`
    /// opens fresh assistant / thinking entries for this turn.
    pub fn begin_user_turn(&mut self, message: impl Into<String>) {
        let text = message.into();
        log::debug!("[tui] state: begin_user_turn len={}", text.len());
        self.entries.push(Entry::new(EntryKind::User, text));
        self.cur_assistant = None;
        self.cur_thinking = None;
        self.streaming = true;
    }

    /// Push a local system/status note (e.g. "Cancelled", connection info).
    pub fn push_system(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::debug!("[tui] state: push_system len={}", text.len());
        self.entries.push(Entry::new(EntryKind::System, text));
    }

    /// Fold one [`WebChannelEvent`] into the transcript.
    ///
    /// Events whose `client_id` does not match ours are ignored (the web-channel
    /// bus is process-wide). Returns nothing; inspect [`Self::entries`] /
    /// [`Self::is_streaming`] afterwards.
    pub fn apply_event(&mut self, ev: &WebChannelEvent) {
        if ev.client_id != self.client_id
            || (!self.thread_id.is_empty() && ev.thread_id != self.thread_id)
        {
            log::trace!(
                "[tui] state: ignoring event={} for other client_id={}",
                ev.event,
                ev.client_id
            );
            return;
        }

        match ev.event.as_str() {
            "text_delta" => {
                if let Some(delta) = ev.delta.as_deref() {
                    self.append_assistant(delta);
                }
            }
            "thinking_delta" => {
                if let Some(delta) = ev.delta.as_deref() {
                    self.append_thinking(delta);
                }
            }
            "tool_call" => {
                let name = ev.tool_name.as_deref().unwrap_or("tool");
                let args = ev.args.as_ref().map(summarize_json).unwrap_or_default();
                log::debug!("[tui] state: tool_call {name}");
                self.entries
                    .push(Entry::new(EntryKind::Tool, format!("→ {name}{args}")));
            }
            "tool_result" => {
                let name = ev.tool_name.as_deref().unwrap_or("tool");
                let ok = ev.success.unwrap_or(true);
                let marker = if ok { "✓" } else { "✗" };
                let detail = ev
                    .output
                    .as_deref()
                    .map(truncate_line)
                    .filter(|s| !s.is_empty())
                    .or_else(|| ev.failure.as_ref().map(summarize_json))
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default();
                log::debug!("[tui] state: tool_result {name} ok={ok}");
                self.entries.push(Entry::new(
                    EntryKind::Tool,
                    format!("{marker} {name}{detail}"),
                ));
            }
            "subagent_spawned"
            | "subagent_iteration_start"
            | "subagent_completed"
            | "subagent_tool_call"
            | "subagent_tool_result" => {
                let agent = ev
                    .subagent
                    .as_ref()
                    .and_then(|detail| detail.display_name.as_deref())
                    .or(ev.tool_name.as_deref())
                    .unwrap_or("sub-agent");
                let action = ev.event.trim_start_matches("subagent_").replace('_', " ");
                self.entries.push(Entry::new(
                    EntryKind::Tool,
                    format!("agent {agent} · {action}"),
                ));
            }
            "artifact_pending" | "artifact_ready" | "artifact_failed" => {
                let status = ev.event.trim_start_matches("artifact_");
                let detail = ev
                    .message
                    .as_deref()
                    .or(ev.output.as_deref())
                    .map(truncate_line)
                    .unwrap_or_default();
                let suffix = if detail.is_empty() {
                    String::new()
                } else {
                    format!(" — {detail}")
                };
                self.entries.push(Entry::new(
                    EntryKind::Tool,
                    format!("artifact · {status}{suffix}"),
                ));
            }
            "chat_done" => {
                log::debug!(
                    "[tui] state: chat_done full_response={}",
                    ev.full_response.is_some()
                );
                // `full_response` is authoritative — it replaces whatever the
                // streamed text deltas accumulated (they can lag / be partial).
                if let Some(full) = ev.full_response.as_deref() {
                    match self.cur_assistant {
                        Some(idx) => self.entries[idx].text = full.to_string(),
                        None => self
                            .entries
                            .push(Entry::new(EntryKind::Assistant, full.to_string())),
                    }
                }
                self.finish_turn();
            }
            "chat_error" => {
                let mut msg = ev.message.as_deref().unwrap_or("Unknown error").to_string();
                if ev.error_retryable == Some(true) {
                    msg.push_str(" · retryable");
                }
                if let Some(delay) = ev.error_retry_after_ms {
                    msg.push_str(&format!(" after {}s", delay.div_ceil(1000)));
                }
                log::debug!("[tui] state: chat_error {msg}");
                self.entries.push(Entry::new(EntryKind::Error, msg));
                self.finish_turn();
            }
            other => {
                log::trace!("[tui] state: unhandled event={other}");
            }
        }
    }

    fn append_assistant(&mut self, delta: &str) {
        match self.cur_assistant {
            Some(idx) => self.entries[idx].text.push_str(delta),
            None => {
                self.entries
                    .push(Entry::new(EntryKind::Assistant, delta.to_string()));
                self.cur_assistant = Some(self.entries.len() - 1);
            }
        }
    }

    fn append_thinking(&mut self, delta: &str) {
        match self.cur_thinking {
            Some(idx) => self.entries[idx].text.push_str(delta),
            None => {
                self.entries
                    .push(Entry::new(EntryKind::Thinking, delta.to_string()));
                self.cur_thinking = Some(self.entries.len() - 1);
            }
        }
    }

    fn finish_turn(&mut self) {
        self.streaming = false;
        self.cur_assistant = None;
        self.cur_thinking = None;
    }

    fn push_projected_item(&mut self, item: &serde_json::Value) {
        match item.get("kind").and_then(serde_json::Value::as_str) {
            Some("userMessage") => self.entries.push(Entry::new(
                EntryKind::User,
                item.get("displayContent")
                    .or_else(|| item.get("content"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )),
            Some("assistantMessage") => self.entries.push(Entry::new(
                EntryKind::Assistant,
                item.get("content")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )),
            Some("reasoning") => self.entries.push(Entry::new(
                EntryKind::Thinking,
                item.get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )),
            Some("toolCall") => {
                let name = item
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("tool");
                let status = item
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("running");
                self.entries
                    .push(Entry::new(EntryKind::Tool, format!("{name} · {status}")));
            }
            Some("interruptedPartial") => self.entries.push(Entry::new(
                EntryKind::Assistant,
                item.get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )),
            Some("subagent") => {
                let id = item
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("sub-agent");
                self.entries
                    .push(Entry::new(EntryKind::Tool, format!("agent {id}")));
                if let Some(items) = item.get("items").and_then(serde_json::Value::as_array) {
                    for nested in items {
                        self.push_projected_item(nested);
                    }
                }
            }
            Some("compaction") => self.entries.push(Entry::new(
                EntryKind::System,
                "Conversation context compacted",
            )),
            _ => {}
        }
    }
}

/// One-line, length-capped summary of a JSON value for tool-call args display.
fn summarize_json(value: &serde_json::Value) -> String {
    let rendered = match value {
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => value.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    format!("({})", truncate_line(&rendered))
}

/// Collapse to a single line and cap length so a rogue tool output can't blow
/// up the transcript width.
fn truncate_line(s: &str) -> String {
    const MAX: usize = 120;
    let single = s.replace(['\n', '\r'], " ");
    let trimmed = single.trim();
    if trimmed.chars().count() > MAX {
        let cut: String = trimmed.chars().take(MAX).collect();
        format!("{cut}…")
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
