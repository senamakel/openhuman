//! Cold-boot resume seeding: priming the next turn's LLM context from an
//! external message log or from the full-fidelity `session_raw` transcript.

use super::super::types::Agent;
use anyhow::Result;

impl Agent {
    /// Seed the next turn's LLM context from an authoritative message
    /// log (e.g. the web channel's per-thread conversation JSONL).
    ///
    /// Mirrors what [`Self::try_load_session_transcript`] does on a
    /// transcript-file hit, but sources from a caller-supplied list so
    /// resume works even when no transcript file exists for this
    /// agent name (the typical situation right after the
    /// `set_agent_definition_name` / `session_key` rename fix landed —
    /// existing transcripts are written under the old name).
    ///
    /// `messages` is `(role, content)` pairs in chronological order.
    /// Recognised roles: `"user"`, `"agent"` / `"assistant"`. Any
    /// trailing user message that exactly matches `current_user_message`
    /// is dropped — the caller is about to pass that text to
    /// [`Self::run_single`], which will append it to history itself, so
    /// keeping it here would duplicate it on the wire.
    ///
    /// No-ops if the agent already has a history or a cached transcript
    /// (i.e. the per-process session cache is warm). Intended only for
    /// cold-boot priming.
    pub fn seed_resume_from_messages(
        &mut self,
        messages: Vec<(String, String)>,
        current_user_message: &str,
    ) -> Result<()> {
        if !self.history.is_empty() || self.cached_transcript_messages.is_some() {
            return Ok(());
        }
        let mut prior = messages;
        if let Some(last) = prior.last() {
            if last.0 == "user" && last.1.trim() == current_user_message.trim() {
                prior.pop();
            }
        }
        if prior.is_empty() {
            return Ok(());
        }

        // Build the system prompt fresh — there's no persisted prefix
        // to preserve here, and learned-context decoration is skipped
        // intentionally so this fallback path stays synchronous and
        // doesn't fan out to the memory store on every cold-boot turn.
        let learned = crate::agent::prompts::LearnedContextData::default();
        let system_prompt = self.build_system_prompt(learned)?;

        let mut cached: Vec<crate::agent::messages::ChatMessage> =
            Vec::with_capacity(prior.len() + 1);
        cached.push(crate::agent::messages::ChatMessage::system(system_prompt));
        for (role, content) in prior {
            let chat = match role.as_str() {
                "user" => crate::agent::messages::ChatMessage::user(content),
                "agent" | "assistant" => crate::agent::messages::ChatMessage::assistant(content),
                // Fall back to user role for unknown senders rather than
                // dropping the message — losing context is worse than
                // mislabelling a system/tool message.
                _ => crate::agent::messages::ChatMessage::user(content),
            };
            cached.push(chat);
        }

        let cached_len_before = cached.len();
        let bounded = self.bound_cached_transcript_messages(cached);
        if bounded.len() < cached_len_before {
            log::warn!(
                "[agent] seed_resume_from_messages — bounded cached transcript {} → {} (max_history_messages={})",
                cached_len_before,
                bounded.len(),
                self.config.max_history_messages
            );
        }
        log::info!(
            "[agent] seed_resume_from_messages — primed cached transcript with {} prior messages",
            bounded.len().saturating_sub(1)
        );
        self.cached_transcript_messages = Some(bounded);
        Ok(())
    }

    /// Cold-boot resume for the web-chat path: pre-populate this session's
    /// LLM context from the **full-fidelity** `session_raw/{stem}.jsonl`
    /// transcript for `thread_id`.
    ///
    /// This is the high-fidelity counterpart to
    /// [`Self::seed_resume_from_messages`]. That fallback sources lossy
    /// `(sender, content)` prose from the conversation log, so it drops every
    /// tool call, tool-role result, and reasoning block — after an app restart
    /// the model then "forgets" all its tool interactions. This path instead
    /// routes thread → transcript via
    /// [`transcript::find_root_transcript_for_thread`] and reuses the exact
    /// [`transcript::read_transcript`] +
    /// [`Self::bound_cached_transcript_messages`] machinery as
    /// [`Self::try_load_session_transcript`], so `tool_calls`, `role:"tool"`
    /// messages, and `reasoning_content` all survive the round-trip. The only
    /// difference from `try_load_session_transcript` is the lookup key (thread
    /// id vs. per-thread agent name), so a thread whose transcript was written
    /// under a differently-scoped agent name still resumes.
    ///
    /// Returns `true` when a transcript was found, loaded, and seeded into
    /// `cached_transcript_messages`; `false` (a no-op) when the agent is already
    /// warm, no root transcript exists for the thread, the transcript is empty,
    /// or it fails to parse — the caller then falls back to prose-pair seeding.
    ///
    /// Best-effort like `try_load_session_transcript`: read/parse failures are
    /// logged and reported as `false` rather than propagated. The current turn's
    /// user message is appended later by [`Self::run_single`] / `turn`, so it is
    /// intentionally absent from the loaded prefix — no dedup is needed here (the
    /// on-disk transcript ends at the previous completed turn).
    ///
    /// Goes through the S4 seam like `try_load_session_transcript` (see its doc
    /// comment for why the read is `read_session` and not
    /// `ChatHistory::messages()`), via the locator's `root_for_thread` — the
    /// lookup that resolves by `_meta.thread_id` across *root* transcripts
    /// only. That disambiguation is why it is a locator method rather than
    /// anything a stem-bound handle could offer: several transcripts share one
    /// thread id (every sub-agent spawned within it does).
    pub fn seed_resume_from_thread_transcript(&mut self, thread_id: &str) -> bool {
        if !self.history.is_empty() || self.cached_transcript_messages.is_some() {
            log::debug!(
                "[web-channel] seed_resume_from_thread_transcript no-op — agent already warm \
                 (history_len={}, cached={}) thread={thread_id}",
                self.history.len(),
                self.cached_transcript_messages.is_some()
            );
            return false;
        }

        // The thread's conversation belongs to the THREAD, not the active
        // profile: the locator resolves cross-dir, newest-wins across the
        // shared `session_raw/` and every profile-scoped `session_raw-<id>/`
        // (#5351), so switching profile mid-thread continues the same
        // conversation. See `FileTranscriptLocator::root_for_thread` for why
        // this must not be own-dir-first.
        let Some(handle) = self.session_locator().root_for_thread(thread_id) else {
            log::debug!(
                "[web-channel] no root session_raw transcript for thread={thread_id} in any \
                 (shared or profile-scoped) session_raw dir — falling back to \
                 conversation-log prose seeding"
            );
            return false;
        };
        let path = handle.path().to_path_buf();

        log::info!(
            "[web-channel] cold-boot resume — loading full-fidelity transcript for \
             thread={thread_id} path={}",
            path.display()
        );

        match handle.read_session() {
            // `Ok(None)` (file vanished between discovery and read) folds into
            // the same empty-transcript branch, so the prose-seeding fallback
            // triggers identically.
            Ok(None) => {
                log::debug!(
                    "[web-channel] root transcript for thread={thread_id} is empty — \
                     falling back to prose seeding"
                );
                false
            }
            Ok(Some(session)) => {
                if session.messages.is_empty() {
                    log::debug!(
                        "[web-channel] root transcript for thread={thread_id} is empty — \
                         falling back to prose seeding"
                    );
                    return false;
                }
                let loaded_count = session.messages.len();
                // Count the tool-role results carried into the resumed prefix —
                // the fidelity the prose fallback would have silently dropped.
                let tool_result_msgs = session.messages.iter().filter(|m| m.role == "tool").count();
                let bounded = self.bound_cached_transcript_messages(session.messages);
                if bounded.len() < loaded_count {
                    log::warn!(
                        "[web-channel] resume prefix trimmed from {} to {} messages \
                         (max_history_messages={}) for thread={thread_id}",
                        loaded_count,
                        bounded.len(),
                        self.config.max_history_messages
                    );
                }
                log::info!(
                    "[web-channel] cold-boot resume — primed {} transcript message(s) \
                     ({} tool-role result(s) preserved) for thread={thread_id}",
                    bounded.len(),
                    tool_result_msgs
                );
                self.cached_transcript_messages = Some(bounded);
                true
            }
            Err(err) => {
                log::warn!(
                    "[web-channel] failed to parse root transcript {} for thread={thread_id}: \
                     {err} — falling back to prose seeding",
                    path.display()
                );
                false
            }
        }
    }
}
