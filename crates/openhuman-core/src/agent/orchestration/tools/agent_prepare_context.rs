//! Tool: `agent_prepare_context` — "plan mode as a subagent".
//!
//! When a parent agent explicitly needs an ad hoc context pass, it can call
//! `agent_prepare_context`. This runs the read-only `context_scout` sub-agent
//! inline (blocking), which gathers context from memory, the user's
//! goals/profile, connected integrations, and the web, then returns a tight
//! `[context_bundle]` envelope: whether there's enough context to act, a
//! compact context summary, and an ordered set of recommended next tool calls
//! drawn from the *parent's own* tool catalogue.
//!
//! The scout's output is bounded by `context_scout`'s `max_result_chars`
//! (≈1000 tokens) so the parent's context only grows by a bounded amount.
//!
//! - [`scout_run`] — the engine: running `context_scout` inline, extracting
//!   its envelope, and classifying/logging failures.
//! - [`tool`] — the `Tool` wrapper: schema and parent-catalogue rendering.

#[path = "agent_prepare_context/scout_run.rs"]
mod scout_run;
#[path = "agent_prepare_context/tool.rs"]
mod tool;

#[cfg(test)]
#[path = "agent_prepare_context_tests.rs"]
mod tests;

pub use scout_run::{run_context_scout, run_context_scout_with_catalog};
pub use tool::AgentPrepareContextTool;

#[cfg(test)]
use scout_run::{
    extract_context_bundle, is_expected_billing_failure, log_scout_failure, scout_failure_signal,
};
