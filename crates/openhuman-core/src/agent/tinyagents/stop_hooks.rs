//! Mid-turn stop hooks as a `tinyagents` middleware (issue #4249).
//!
//! openhuman's policy-driven [`StopHook`]s (budget cap, thread-goal budget,
//! ad-hoc iteration ceiling) used to fire between iterations of the legacy
//! tool-call loop. That loop is gone — every turn now runs on the `tinyagents`
//! harness — so the firing point moves into a [`Middleware`]:
//!
//! - [`Middleware::after_model`] accumulates each completed model call's usage
//!   into a per-turn [`TurnCost`] (the same accounting the hooks read), then
//!   evaluates every installed hook with a [`TurnState`] snapshot.
//! - On the first [`StopDecision::Stop`], it sends [`SteeringCommand::Pause`] on
//!   the run's steering handle. The agent loop drains steering at the **top** of
//!   the next iteration (before the next model call) and `Pause` short-circuits
//!   it cleanly — so the run stops before spending another call, returning the
//!   partial transcript. This mirrors the [`CapPauser`](super::CapPauser)
//!   model-call-cap mechanism.
//!
//! The hook list (the goal-budget hook) is handed to
//! [`StopHookMiddleware::new`]; the middleware is only registered when the list
//! is non-empty.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tinyagents_harness::context::RunContext;
use tinyagents_harness::middleware::Middleware;
use tinyagents_harness::steering::{SteeringCommand, SteeringHandle};
use tinyinference_llm::model::ModelResponse;

use crate::agent::cost::TurnCost;
use crate::agent::stop_hooks::{StopDecision, StopHook, TurnState};

/// Install after-call usage policies on the assembled turn harness.
pub(super) fn install(
    harness: &mut tinyagents_harness::runtime::AgentHarness<(), super::host::OpenHumanRunContext>,
    handle: Option<&SteeringHandle>,
    model: &str,
    max_iterations: usize,
    hooks: Vec<Arc<dyn StopHook>>,
    halt_summary: super::HaltSummarySlot,
) {
    if let Some(handle) = handle.filter(|_| !hooks.is_empty()) {
        harness.push_middleware(Arc::new(StopHookMiddleware::new(
            handle.clone(),
            model,
            max_iterations,
            hooks,
            halt_summary,
        )));
    }
}

/// Fires openhuman [`StopHook`]s after each model call and pauses the run when
/// any hook votes to stop.
pub(super) struct StopHookMiddleware {
    /// Steering handle the run was built with; `Pause` is sent here on stop.
    handle: SteeringHandle,
    /// Model name reported to hooks (and used for cost estimation).
    model: String,
    /// Configured model-call ceiling for this turn (the hooks' `max_iterations`).
    max_iterations: u32,
    /// 0-based count of completed model calls; incremented in `after_model`.
    iteration: AtomicU32,
    /// Per-turn usage tally the hooks read (`TurnState::cost`).
    cost: Mutex<TurnCost>,
    /// Installed hooks, snapshotted from the task-local at construction.
    hooks: Vec<Arc<dyn StopHook>>,
    /// Latches once a hook has voted to stop, so we send `Pause` exactly once.
    stopped: AtomicBool,
    halt_summary: super::HaltSummarySlot,
}

impl StopHookMiddleware {
    /// Build a middleware firing `hooks`, pausing `handle` on the first stop.
    pub(super) fn new(
        handle: SteeringHandle,
        model: impl Into<String>,
        max_iterations: usize,
        hooks: Vec<Arc<dyn StopHook>>,
        halt_summary: super::HaltSummarySlot,
    ) -> Self {
        Self {
            handle,
            model: model.into(),
            max_iterations: max_iterations.min(u32::MAX as usize) as u32,
            iteration: AtomicU32::new(0),
            cost: Mutex::new(TurnCost::new()),
            hooks,
            stopped: AtomicBool::new(false),
            halt_summary,
        }
    }
}

#[async_trait]
impl<State, Ctx> Middleware<State, Ctx> for StopHookMiddleware
where
    State: Send + Sync,
    Ctx: Send + Sync,
{
    fn name(&self) -> &str {
        "openhuman.stop_hooks"
    }

    async fn after_model(
        &self,
        _ctx: &mut RunContext<Ctx>,
        _state: &State,
        response: &mut ModelResponse,
    ) -> tinyagents_harness::Result<()> {
        // Already paused: nothing more to do (Pause is latched once).
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Fold this call's usage into the running turn cost (the same tally the
        // budget/goal hooks read). Clone it back out so we never hold the mutex
        // guard across the async hook checks below.
        let iteration = self.iteration.fetch_add(1, Ordering::SeqCst) + 1;
        let cost_snapshot = {
            let mut cost = self.cost.lock().expect("stop-hook cost mutex poisoned");
            if let Some(usage) = super::model::usage_info_from_response(response) {
                cost.add_call(&self.model, &usage);
            }
            cost.clone()
        };

        let turn_state = TurnState {
            iteration,
            max_iterations: self.max_iterations,
            cost: &cost_snapshot,
            model: &self.model,
        };

        // Every observer sees the completed call, including observers installed
        // after a budget policy. Preserve the first stop reason for the result.
        let mut stop = None;
        for hook in &self.hooks {
            if let StopDecision::Stop { reason } = hook.check(&turn_state).await {
                if stop.is_none() {
                    stop = Some((hook.name().to_owned(), reason));
                }
            }
        }
        if let Some((hook, reason)) = stop {
            if self.stopped.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            tracing::warn!(
                target: "stop_hooks",
                hook,
                iteration,
                model = %self.model,
                "[stop_hooks] hook voted to stop the turn — pausing run: {reason}"
            );
            *self
                .halt_summary
                .lock()
                .expect("stop-hook halt mutex poisoned") = Some(format!(
                "Stopping after {} model call(s): {reason}",
                iteration
            ));
            self.handle.send(SteeringCommand::Pause);
        }

        Ok(())
    }
}
