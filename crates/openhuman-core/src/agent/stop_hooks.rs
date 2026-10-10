//! Mid-turn stop hooks — policy-driven halt of an in-flight agent
//! turn.
//!
//! Stop hooks are the policy lever: budget caps, rate limits, custom kill
//! switches. They run between iterations of the agent loop so a runaway turn can
//! be cut short before the next provider call rather than after the fact.
//! (User-driven cancellation — Ctrl+C / `/stop` — is handled separately by the
//! `tinyagents` steering/cancellation channel.)
//!
//! The `tinyagents` adapter installs a `StopHookMiddleware`
//! ([`crate::agent::tinyagents::stop_hooks`]) that fires each hook after
//! every model call; a hook returning [`StopDecision::Stop`] pauses the run
//! gracefully (via the steering handle) before the next provider call. The
//! goal-budget hook ([`crate::agent::goals::runtime::GoalBudgetStopHook`]) is
//! the live implementation.
//!
//! The per-turn tool budget ([`with_tool_call_limit`]) also lives here, on a
//! task-local.

use crate::agent::cost::TurnCost;
use async_trait::async_trait;
use std::sync::Arc;

/// A policy hook fired between iterations of the tool-call loop.
#[async_trait]
pub trait StopHook: Send + Sync {
    /// Stable name for tracing / error messages (e.g. `"budget"`).
    fn name(&self) -> &str;

    /// Inspect the current turn state and decide whether to continue.
    async fn check(&self, ctx: &TurnState<'_>) -> StopDecision;
}

/// Outcome of a single hook check.
#[derive(Debug, Clone)]
pub enum StopDecision {
    /// Keep the loop running.
    Continue,
    /// Stop the loop. `reason` is propagated to the caller.
    Stop { reason: String },
}

/// Snapshot of the turn at the moment a hook fires. References are
/// borrowed from the loop's locals so hooks pay no allocation cost on
/// the hot path; clone fields out if you need to keep them.
pub struct TurnState<'a> {
    /// 1-based iteration index that's about to start.
    pub iteration: u32,
    /// Configured iteration cap for this turn.
    pub max_iterations: u32,
    /// Cumulative cost / token tally so far.
    pub cost: &'a TurnCost,
    /// Model name passed to this turn's provider calls.
    pub model: &'a str,
}

tokio::task_local! {
    static CURRENT_STOP_HOOKS: Vec<Arc<dyn StopHook>>;
}

/// Clone the stop hooks active in the current task, if any.
pub fn current_stop_hooks() -> Vec<Arc<dyn StopHook>> {
    CURRENT_STOP_HOOKS
        .try_with(Clone::clone)
        .unwrap_or_default()
}

/// Scope a future with per-turn stop hooks.
pub async fn with_stop_hooks<F: std::future::Future>(
    hooks: Vec<Arc<dyn StopHook>>,
    future: F,
) -> F::Output {
    CURRENT_STOP_HOOKS.scope(hooks, future).await
}

/// Stop a turn when its cumulative cost reaches the configured USD cap.
#[derive(Debug, Clone, Copy)]
pub struct BudgetStopHook {
    /// Cumulative USD charge at which the turn pauses.
    pub max_usd: f64,
}

impl BudgetStopHook {
    /// Construct a hook that pauses after a turn reaches `max_usd`.
    pub fn new(max_usd: f64) -> Self {
        Self { max_usd }
    }
}

#[async_trait]
impl StopHook for BudgetStopHook {
    fn name(&self) -> &str {
        "budget"
    }

    async fn check(&self, ctx: &TurnState<'_>) -> StopDecision {
        if !self.max_usd.is_finite() || self.max_usd <= 0.0 {
            return StopDecision::Stop {
                reason: format!("invalid budget cap configured: max_usd={}", self.max_usd),
            };
        }
        // A cap can only bite on spend it can see: calls of unknown cost add
        // nothing here rather than a made-up rate.
        let spent = ctx.cost.cost.known_usd;
        if spent >= self.max_usd {
            StopDecision::Stop {
                reason: format!("turn cost ${spent:.4} reached cap ${:.4}", self.max_usd),
            }
        } else {
            StopDecision::Continue
        }
    }
}

tokio::task_local! {
    static CURRENT_TOOL_CALL_LIMIT: usize;
}

/// Narrow the real tool invocation budget for one turn, including parallel calls.
/// Nested scopes cannot widen their parent's budget. The scope resets on exit.
pub async fn with_tool_call_limit<F: std::future::Future>(
    limit: Option<usize>,
    future: F,
) -> F::Output {
    let inherited = CURRENT_TOOL_CALL_LIMIT.try_with(|n| *n).ok();
    tracing::debug!(
        limit,
        inherited,
        "[tinyagents] installing per-turn tool budget"
    );
    match (limit, inherited) {
        (Some(a), Some(b)) => CURRENT_TOOL_CALL_LIMIT.scope(a.min(b), future).await,
        (Some(n), None) | (None, Some(n)) => CURRENT_TOOL_CALL_LIMIT.scope(n, future).await,
        (None, None) => future.await,
    }
}

pub(crate) fn tool_call_limit(max_iterations: usize) -> usize {
    let default = max_iterations.saturating_mul(8).max(8);
    CURRENT_TOOL_CALL_LIMIT
        .try_with(|n| default.min(*n))
        .unwrap_or(default)
}

#[cfg(test)]
#[path = "stop_hooks_tests.rs"]
mod tests;
