use super::super::store::TurnStateStore;
use super::super::types::TurnState;

/// In-process cursor that keeps the authoritative [`TurnState`] in sync
/// with the agent loop and writes it through to a [`TurnStateStore`].
pub struct TurnStateMirror {
    pub(super) store: TurnStateStore,
    pub(super) state: TurnState,
    /// Set to `true` once we observe `TurnCompleted` so `finish` knows
    /// to delete the snapshot rather than mark it interrupted.
    pub(super) turn_completed: bool,
    /// Monotonic ordering key for [`TranscriptItem`]s. Round alone can't
    /// order narration vs thinking vs tool calls *within* one iteration, so
    /// every transcript push stamps and increments this.
    pub(super) next_seq: u32,
    /// Separate monotonic ordering key for [`ToolTimelineEntry::seq`] — the flat
    /// timeline is an independent projection from the interleaved transcript, so
    /// it gets its own space (sharing `next_seq` would leave gaps in the
    /// transcript's contiguous ordering).
    pub(super) next_tool_seq: u64,
}

impl TurnStateMirror {
    /// Build a mirror primed with a `Started` snapshot and immediately
    /// flush so a crash before the first agent event still leaves a
    /// recoverable record.
    pub fn new(
        store: TurnStateStore,
        thread_id: impl Into<String>,
        request_id: impl Into<String>,
    ) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        let state = TurnState::started(thread_id, request_id, 0, now);
        let mut mirror = Self {
            store,
            state,
            turn_completed: false,
            next_seq: 0,
            next_tool_seq: 0,
        };
        mirror.flush();
        mirror
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> &TurnState {
        &self.state
    }
}
