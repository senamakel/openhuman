//! Restart-survivable snapshots of in-flight agent turns.
//!
//! [`store::TurnStateStore`] is the persistence layer: one JSON file per
//! turn under the workspace, written atomically and serialized through a
//! process-wide mutex; `mark_all_interrupted` flags any non-terminal
//! snapshot left over from an unclean shutdown at cold boot.
//! [`mirror::TurnStateMirror`] is the writer — it translates
//! [`crate::agent::progress::AgentProgress`] events into [`types::TurnState`]
//! mutations and flushes to the store at iteration / tool boundaries (not on
//! every streaming delta), keeping `Completed` snapshots so the UI can replay
//! a finished turn.
//!
//! [`types`] holds the wire/storage shapes; they are camelCase to mirror
//! `app/src/store/chatRuntimeSlice.ts`. `TurnStateMirror` is driven by the
//! web-channel progress bridge (`web_chat::progress_bridge`), and snapshots
//! are read back through the `threads` RPC surface (`turn_state_get` /
//! `turn_state_list` / `turn_state_history` / `turn_state_get_turn` /
//! `turn_state_clear`). See `../README.md` for the full `threads` module
//! picture.

pub mod mirror;
pub mod store;
pub mod types;

pub use mirror::TurnStateMirror;

pub use store::TurnStateStore;
pub use types::{
    ClearTurnStateRequest, ClearTurnStateResponse, GetTurnStateForRequestRequest,
    GetTurnStateRequest, GetTurnStateResponse, ListTurnStatesResponse, SubagentActivity,
    SubagentToolCall, ToolTimelineEntry, ToolTimelineStatus, TurnLifecycle, TurnPhase, TurnState,
};
