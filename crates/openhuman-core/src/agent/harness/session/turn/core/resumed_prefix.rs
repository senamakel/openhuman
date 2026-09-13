//! Folding a resumed session's replayed transcript prefix into the live history.

use crate::agent::harness::session::types::Agent;
use crate::agent::messages::ConversationMessage;

impl Agent {
    /// Fold a resumed session's replayed transcript prefix into [`Agent::history`].
    ///
    /// A no-op unless `cached_transcript_messages` is set, which happens only on
    /// the first turn after a resume.
    pub(in crate::agent::harness::session) fn absorb_resumed_transcript_prefix(&mut self) {
        let Some(cached) = self.cached_transcript_messages.take() else {
            return;
        };
        // A resumed session's replayed prefix is **absorbed into
        // `self.history`**, not merely prepended to this one request.
        //
        // It used to be prepended: the request was built from `history`,
        // the cached prefix was `take()`n and spliced in front, and
        // `history` was left holding only the new turn. That made the
        // replayed conversation visible to exactly one request, and it cost
        // the conversation twice over — both observed on the wire, not
        // reasoned about:
        //
        //  1. **The next turn lost it.** `take()` empties the field, and
        //     nothing else ever re-read the transcript, so turn 2 after a
        //     restart went out with only its own messages. A ten-message
        //     thread answered its next question from four. That is
        //     conversational amnesia first and a cache miss second.
        //  2. **The transcript written afterwards lost it.** Persistence
        //     serializes `history` (see `run_turn_impl`'s
        //     `persist_session_transcript` call), so the file produced after
        //     a resume held only the new turn — and `latest_for_agent` hands
        //     that file to the *next* resume. Each restart truncated the
        //     thread a little further.
        //
        // Absorbing it makes `history` the single source of truth again, so
        // the request, the following turn and the durable record cannot
        // disagree. The bytes on the wire for *this* request are unchanged:
        // the prefix's entries are already provider-shaped `ChatMessage`s
        // and `Chat` is rendered verbatim, exactly as the splice did.
        //
        // The leading system message(s) built for this turn are dropped in
        // favour of the prefix's own, which is the whole point of resuming —
        // the stored prompt is the KV-cache prefix the backend has already
        // tokenised, and re-rendering it is what this path exists to avoid.
        //
        // `Chat` rather than the structured `AssistantToolCalls` /
        // `ToolResults` variants is deliberate: these entries have already
        // been through `bound_cached_transcript_messages`, which snaps the
        // window past an orphaned lead and strips a trailing unpaired
        // opener, so the cycle-pairing guard has nothing left to do. Lifting
        // them into the structured variants would mean re-deriving a pairing
        // that was already decided, with a second chance to get it wrong;
        // the native envelope they carry is parsed back into real tool calls
        // at the provider seam either way (see
        // `message_convert::parse_native_assistant_envelope`, which exists
        // for precisely this seeded-transcript case).
        // `bound_cached_transcript_messages` preserves a leading system
        // message only "when present" — a resumed transcript that was never
        // seeded with one (or was truncated ahead of it) hands back a
        // `cached` prefix with no system entry at all. Dropping this turn's
        // freshly-built system message(s) unconditionally would then leave
        // the absorbed history with none, not the cached one's — so only
        // drop them when the cached prefix actually supplies a replacement.
        let cached_has_system = matches!(cached.first(), Some(msg) if msg.role == "system");
        let tail: Vec<ConversationMessage> = self
            .history
            .drain(..)
            .skip_while(|entry| {
                cached_has_system
                    && matches!(entry, ConversationMessage::Chat(chat) if chat.role == "system")
            })
            .collect();
        self.history = cached
            .into_iter()
            .map(ConversationMessage::Chat)
            .chain(tail)
            .collect();
    }
}
