//! Loading a prior session transcript for KV-cache resume, and the locator
//! that finds it.

use crate::agent::harness::session::transcript_history::{
    FileTranscriptLocator, SessionHistoryLocator,
};
use crate::agent::harness::session::types::Agent;

impl Agent {
    // ─────────────────────────────────────────────────────────────────
    // Session transcript helpers
    // ─────────────────────────────────────────────────────────────────

    /// Try to load a previous session transcript for KV cache resume.
    ///
    /// Best-effort: failures are logged and silently ignored.
    ///
    /// # How this reaches the transcript (S4)
    ///
    /// Both halves of the turn path now go through the seam: writes through
    /// [`SessionHistory::append_turn`][crate::agent::harness::session::transcript_history::SessionHistory::append_turn],
    /// reads through
    /// [`SessionHistoryLocator`][crate::agent::harness::session::transcript_history::SessionHistoryLocator]
    /// + [`SessionTranscriptRead::read_session`][crate::agent::harness::session::transcript_history::SessionTranscriptRead::read_session].
    ///
    /// The read is **not** `ChatHistory::messages()`, and that is settled, not
    /// pending: `messages()` returns `Vec<Message>`, and converting back with
    /// `message_to_chat_message` flattens `Assistant.tool_calls` into plain
    /// text. That is precisely what
    /// [`bound_cached_transcript_messages`][Agent::bound_cached_transcript_messages]'
    /// TAURI-RUST-7 trailing strip inspects, and re-sending a flattened prefix
    /// to a native provider is the `400 assistant message with 'tool_calls'
    /// must be followed by tool messages` failure that strip exists to prevent.
    /// `read_session` returns the whole [`SessionTranscript`][crate::agent::harness::session::transcript::SessionTranscript]
    /// instead — the same struct the free function returns, from the same
    /// `read_transcript` call — so compaction replay, `interrupted: true`
    /// partial skipping and the `_meta` header
    /// [`maybe_shadow_read_session_store`][Agent::maybe_shadow_read_session_store]
    /// needs all survive by construction.
    ///
    /// Discovery lives on the locator because it is a *lookup*, not a read:
    /// this function's key is `(workspace, session_raw_subdir, agent name)`
    /// (newest match, with a legacy `session_raw/DDMMYYYY/` fallback) and the
    /// cold-boot sibling
    /// [`seed_resume_from_thread_transcript`][Agent::seed_resume_from_thread_transcript]
    /// keys off `_meta.thread_id`. Neither is a stem, and `ChatHistory` has no
    /// discovery concept at all.
    pub(crate) fn try_load_session_transcript(&mut self) {
        let Some(handle) = self
            .session_locator()
            .latest_for_agent(&self.agent_definition_name)
        else {
            log::debug!(
                "[transcript] no previous transcript found for agent={}",
                self.agent_definition_name
            );
            return;
        };
        let path = handle.path().to_path_buf();
        log::info!(
            "[transcript] found previous transcript path={}",
            path.display()
        );
        match handle.read_session() {
            // `Ok(None)` (file vanished between discovery and read) folds into
            // the same "nothing to resume from" branch as an empty transcript,
            // so the caller's behaviour is unchanged either way.
            Ok(None) => {
                log::debug!("[transcript] previous transcript is empty — skipping resume");
            }
            Ok(Some(session)) => {
                if session.messages.is_empty() {
                    log::debug!("[transcript] previous transcript is empty — skipping resume");
                    return;
                }
                let loaded_count = session.messages.len();
                log::info!("[transcript] loaded {} messages for resume", loaded_count);
                // Best-effort store-backed shadow read (issue #4249,
                // 04.2 phase 2). Observes + logs divergence only; the
                // legacy transcript just loaded stays authoritative and
                // is what feeds the resume below. Gated OFF by default.
                self.maybe_shadow_read_session_store(&path, &session);
                let bounded = self.bound_cached_transcript_messages(session.messages);
                if bounded.len() < loaded_count {
                    log::warn!(
                        "[transcript] resume prefix trimmed from {} to {} messages (max_history_messages={})",
                        loaded_count,
                        bounded.len(),
                        self.config.max_history_messages
                    );
                }
                self.cached_transcript_messages = Some(bounded);
            }
            Err(err) => {
                log::warn!(
                    "[transcript] failed to parse previous transcript {}: {err}",
                    path.display()
                );
            }
        }
    }

    /// The transcript locator for this session — the injected one, or a
    /// [`FileTranscriptLocator`] built from the agent's **current** workspace.
    ///
    /// Built per call rather than cached: `workspace_dir` and
    /// `session_raw_subdir` are reassignable after `build()` (tests do exactly
    /// that), and a locator frozen at build time would silently keep resolving
    /// against the directory the agent no longer uses. The construction is two
    /// clones of small strings — cheaper than the `read_dir` it precedes.
    pub(crate) fn session_locator(&self) -> std::sync::Arc<dyn SessionHistoryLocator> {
        match &self.session_history_locator {
            Some(locator) => locator.clone(),
            None => std::sync::Arc::new(FileTranscriptLocator::new(
                self.workspace_dir.clone(),
                self.session_raw_subdir.clone(),
            )),
        }
    }
}
