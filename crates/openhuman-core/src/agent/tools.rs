//! Agent-owned dialogue and control tools.
//!
//! These tools act on the agent loop, its task board, or the user's stored
//! preferences rather than on files, memory, or the network. Wire names are
//! given in parentheses where they differ from the type name:
//!
//! - [`AskClarificationTool`] (`ask_user_clarification`) — returns the
//!   question as its output; the turn actually pauses only because callers
//!   list this name in the harness seam's `early_exit_tools`.
//! - [`DelegateTool`] — hands a subtask to a named agent with its own
//!   provider/model configuration. [`DelegateToPersonalityTool`] does the
//!   same for a named personality.
//! - [`PlanExitTool`] — ends a plan-mode pass by returning the plan plus
//!   [`PLAN_EXIT_MARKER`]. The mode switch itself lives outside the tool;
//!   nothing in this crate consumes the marker yet.
//! - [`RememberPreferenceTool`] — pins an explicit `(class, key, value)`
//!   preference into the `user_profile` memory namespace.
//!   [`SavePreferenceTool`] stores a free-form preference in either the
//!   `general` or `situational` lane.
//! - `RunWorkflowTool` / `AwaitWorkflowTool` — spawn a
//!   `crate::skills::runtime` workflow run and wait on its outcome. Compiled
//!   in only with the `skills` feature, so builds without it omit both tools
//!   from the catalog.
//! - [`TodoTool`] — CRUD on the current thread's task board.
//!   [`UpdateTaskTool`] edits one card by id on a target board (default:
//!   the proactive `task-sources` board).
//!
//! `crate::tools` re-exports everything here (`pub use
//! crate::agent::tools::*;` in `tools/mod.rs`); `tools::ops` registers the
//! tools into the catalog.
mod ask_clarification;
mod delegate;
mod delegate_to_personality;
mod plan_exit;
pub mod remember_preference;
// Pure `skill_runtime` client (spawn + await a workflow run) — compiled out
// with the `skills` gate so the tool list OMITS these rather than degrading
// them to a disabled-error.
#[cfg(feature = "skills")]
mod run_workflow;
pub mod save_preference;
mod todo;
mod update_task;

pub use ask_clarification::AskClarificationTool;
pub use delegate::DelegateTool;
pub use delegate_to_personality::DelegateToPersonalityTool;
pub use plan_exit::{PlanExitTool, PLAN_EXIT_MARKER};
pub use remember_preference::RememberPreferenceTool;
#[cfg(feature = "skills")]
pub use run_workflow::{
    AwaitWorkflowTool, RunWorkflowTool, AWAIT_WORKFLOW_TOOL_NAME, RUN_WORKFLOW_TOOL_NAME,
};
pub use save_preference::SavePreferenceTool;
pub use todo::TodoTool;
pub use update_task::UpdateTaskTool;
