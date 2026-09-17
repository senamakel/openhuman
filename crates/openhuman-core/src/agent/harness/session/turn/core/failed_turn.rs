//! Keeping what a failed chat turn did (#6281).

use crate::agent::harness::session::transcript::{MessageUsage, TurnUsage};
use crate::agent::harness::session::turn_checkpoint::{truncate_chars, CHECKPOINT_RESULT_CHARS};
use crate::agent::harness::session::types::Agent;
use crate::agent::harness::tool_result_artifacts::ToolResultArtifactStore;
use crate::agent::messages::{ChatMessage, ConversationMessage};
use crate::agent::tinyagents::{
    render_unanswered_steps, TranscriptSnapshot, TranscriptSnapshotSink,
};
use anyhow::Result;
use tinyinference::message::Message;

/// Leads the note a failed turn leaves in history, so the model (and a reader
/// of the transcript) can tell it from a reply.
const FAILED_TURN_NOTE_PREFIX: &str = "[turn failed before completion:";

impl Agent {
    /// Drive the chat turn and, if it fails, keep what it did.
    ///
    /// `turn()` has already pushed this turn's user message, but on `Err` the
    /// turn body returned before anything else it produced reached `history` or
    /// the transcript. Every follow-up ("what happened?", "try again") then ran
    /// without the tool calls, their results, or the reason the turn failed.
    ///
    /// On any error from the turn body (provider, harness, tool loop, or a typed
    /// error raised after the loop) this appends the rounds the provider had
    /// already accepted, then a failure note, and persists the transcript, so a
    /// warm follow-up and a cold-boot resume both see them. A turn future that is
    /// dropped (cancelled) is not an error and records nothing here.
    pub(super) async fn run_turn_via_tinyagents_session(
        &mut self,
        user_message: &str,
        effective_model: &str,
        temperature: f64,
        max_iterations: usize,
        artifact_store: Option<ToolResultArtifactStore>,
        suppress_tools: bool,
    ) -> Result<String> {
        let snapshot = TranscriptSnapshotSink::default();
        let result = Box::pin(self.run_turn_via_tinyagents_session_inner(
            user_message,
            effective_model,
            temperature,
            max_iterations,
            artifact_store,
            suppress_tools,
            snapshot.clone(),
        ))
        .await;
        if let Err(err) = &result {
            let snapshot = std::mem::take(
                &mut *snapshot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            self.record_failed_turn(&snapshot, effective_model, err);
        }
        result
    }

    fn record_failed_turn(
        &mut self,
        snapshot: &TranscriptSnapshot,
        effective_model: &str,
        err: &anyhow::Error,
    ) {
        let base = snapshot.request_base_len.min(snapshot.messages.len());
        let accepted_end = snapshot.accepted_end();
        let accepted = &snapshot.messages[base..accepted_end];
        let unanswered = &snapshot.messages[accepted_end..];
        log::warn!(
            "[agent_loop] turn failed; recording {} accepted round message(s), {} unanswered \
             message(s) as text, and the failure cause into history session_id={} \
             request_id={} model={} — #6281",
            accepted.len(),
            unanswered.len(),
            self.event_session_id,
            crate::agent::turn_origin::current_request_id()
                .as_deref()
                .unwrap_or("-"),
            effective_model
        );
        // Only rounds the provider already answered are replayed as structured
        // messages. The newest round was carried only by the failing request, and
        // a request rejected for malformed tool history must not be replayed, or
        // the de-poison eviction in `web_chat::run_task` would reseed the same
        // rejection from this transcript. That round goes into the note as text.
        self.history
            .extend(crate::agent::message_convert::messages_to_conversation(
                accepted,
            ));
        self.history
            .push(ConversationMessage::Chat(ChatMessage::assistant(
                failed_turn_note(err, unanswered),
            )));
        self.trim_history();

        // The calls the provider answered were billed even though the turn
        // failed. Their spend already reached the cost tracker live (the event
        // bridge records usage per call); the transcript records the same totals
        // instead of zeros.
        let (input, output, cached) = (
            snapshot.input_tokens,
            snapshot.output_tokens,
            snapshot.cached_input_tokens,
        );
        let cost_usd = crate::platform::cost::catalog::estimate_cost_usd(
            effective_model,
            input,
            output,
            cached,
        );
        let persisted = self.tool_dispatcher.to_provider_messages(&self.history);
        let turn_usage = TurnUsage {
            provider: self.event_channel().to_string(),
            model: effective_model.to_string(),
            usage: MessageUsage {
                input,
                output,
                cached_input: cached,
                context_window: 0,
                cost_usd,
            },
            ts: chrono::Utc::now().to_rfc3339(),
            reasoning_content: None,
            tool_calls: Vec::new(),
            iteration: snapshot.model_calls,
        };
        self.persist_session_transcript(
            &persisted,
            input,
            output,
            cached,
            cost_usd,
            Some(&turn_usage),
        );
    }
}

/// The failure cause, plus the unanswered steps rendered as text.
fn failed_turn_note(err: &anyhow::Error, unanswered: &[Message]) -> String {
    let cause = format!(
        "{FAILED_TURN_NOTE_PREFIX} {}]",
        truncate_chars(&err.to_string(), CHECKPOINT_RESULT_CHARS)
    );
    match render_unanswered_steps(unanswered) {
        Some(steps) => format!("{cause}\n\n{steps}"),
        None => cause,
    }
}
