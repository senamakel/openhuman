//! Close a turn that has no usable reply of its own from its tool records: a
//! tool turn that ended without final text (#4093, #6278) or a run the
//! no-progress breaker halted (#6279).

use crate::agent::harness::session::turn_checkpoint::{self, CloseVerdict};
use crate::agent::harness::session::types::Agent;
use crate::agent::messages::{ChatMessage, ConversationMessage};
use crate::agent::tinyagents::TinyagentsTurnOutcome;
use crate::inference::provider::UsageInfo;

impl Agent {
    /// Write the closing message for a turn whose run produced no usable reply:
    /// the loop ended on empty text after tool work (#4093), or the breaker
    /// halted it (`outcome.breaker_halt`, #6279).
    ///
    /// Three steps, each closing a way the old path shipped a non-answer:
    ///
    /// 1. **Grounded wrap-up.** One tools-disabled call whose instruction
    ///    restates this turn's tool records, failures and their messages
    ///    included, plus the breaker's stop note as input. Context middleware
    ///    may have cleared or summarised those bodies earlier in the turn, and a
    ///    model that cannot see a success will say it never happened (#6278).
    /// 2. **Check.** A separate call that sees only the request, the records and
    ///    the candidate rejects a reply that narrates intent, contradicts a
    ///    record, or drops the failure that explains an unfinished request.
    /// 3. **Fallback.** An empty, tool-calling, rejected or unverified close
    ///    (the check failed or gave no verdict) is replaced by
    ///    [`turn_checkpoint::build_deterministic_final_summary`], which quotes
    ///    each result and the stop note.
    ///
    /// Accepted model text is streamed only after the check, so a rejected
    /// reply never renders and the streamed text matches the persisted one. The
    /// reply is pushed onto `history`, and the usage of every extra call is
    /// returned for the caller's turn accounting.
    pub(super) async fn close_turn_from_records(
        &mut self,
        outcome: &TinyagentsTurnOutcome,
        user_message: &str,
        effective_model: &str,
    ) -> (String, Vec<UsageInfo>) {
        let stop_reason = outcome.breaker_halt.as_deref();
        let mut usage = Vec::new();

        // The run folded its blank terminal assistant response into history (an
        // empty `Chat(assistant(""))`). Drop it before the wrap-up request and
        // before the reply is appended: strict providers reject empty content,
        // and the transcript must not carry a dangling blank turn.
        if self
            .history
            .last()
            .is_some_and(super::is_empty_assistant_chat)
        {
            self.history.pop();
        }

        let results = super::checkpoint_results_from_conversation(
            &outcome.conversation,
            &outcome.tool_outcomes,
        );
        let records =
            turn_checkpoint::render_tool_results(&results, turn_checkpoint::GROUNDING_TOTAL_CHARS);
        let iteration = outcome.model_calls as u32 + 1;

        let base = self.tool_dispatcher.to_provider_messages(&self.history);
        let (candidate, candidate_usage) = self
            .summarize_turn_wrapup(
                &base,
                effective_model,
                iteration,
                &turn_checkpoint::final_answer_instruction(stop_reason, &records),
                false,
            )
            .await;
        usage.extend(candidate_usage);

        let verdict = if candidate.trim().is_empty() {
            None
        } else {
            let prompt =
                turn_checkpoint::close_verification_prompt(user_message, &records, &candidate);
            let (verdict_text, verdict_usage) = self
                .silent_completion(
                    &[ChatMessage::user(prompt)],
                    effective_model,
                    "closing-message check",
                )
                .await;
            usage.extend(verdict_usage);
            Some(turn_checkpoint::parse_close_verdict(&verdict_text))
        };
        if verdict == Some(CloseVerdict::Unclear) {
            log::warn!(
                "[agent_loop] closing-message check returned no ACCEPT/REJECT verdict; using the deterministic fallback"
            );
        }

        // Only an explicit ACCEPT ships the model's text. A failed or malformed
        // check leaves the reply unverified, and shipping unverified closing text
        // is the defect this path exists to stop (CodeRabbit on #6289).
        let accepted = verdict == Some(CloseVerdict::Accept);
        let reply = if accepted {
            self.stream_text_continuation(&candidate, iteration).await;
            candidate
        } else {
            turn_checkpoint::build_deterministic_final_summary(&results, stop_reason)
        };

        log::info!(
            "[agent_loop] closed turn from tool records after {} tool call(s): halted={} verdict={:?} fallback={} ({} chars) — #4093 #6278 #6279",
            outcome.tool_calls,
            stop_reason.is_some(),
            verdict,
            !accepted,
            reply.chars().count()
        );

        self.history
            .push(ConversationMessage::Chat(ChatMessage::assistant(
                reply.clone(),
            )));
        (reply, usage)
    }
}
