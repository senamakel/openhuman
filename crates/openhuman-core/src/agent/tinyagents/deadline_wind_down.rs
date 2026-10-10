//! Wind a top-level turn down before the outer backstop drops it.
//!
//! [`TurnDeadline`] puts the wind-down point at 780s under the default 900s
//! web backstop. Once that point has passed, [`DeadlineWindDownMiddleware`]
//! sends [`SteeringCommand::Pause`] at the next safe checkpoint: after a model
//! call (that call's tool round still runs) or after a tool result (so a long
//! sub-agent call is not followed by yet another model call). The loop drains
//! the pause at the top of its next iteration, before any provider call. That
//! is the same mechanism as the goal-budget and cap pauses
//! ([`super::stop_hooks`]).
//!
//! A paused run has no final response, so the session driver's grounded close
//! (`session_host::driver::grounded_close`) writes the user's answer from the
//! completed tool results. The turn then commits like any other, and the user
//! gets an answer instead of a dropped turn.
//!
//! [`install`] also clamps the harness `max_wall_clock_ms` to the deadline's
//! hard stop. A single tool call that runs past the wind-down point is then
//! ended by the harness's own typed `Timeout`, before the backstop drops the
//! turn with nothing recorded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tinyagents_harness::context::RunContext;
use tinyagents_harness::middleware::{Middleware, ToolInvocationIdentity};
use tinyagents_harness::runtime::AgentHarness;
use tinyagents_harness::steering::{SteeringCommand, SteeringHandle};
use tinyinference_llm::model::ModelResponse;
use tinytools::ToolResult;

use crate::agent::tinyagents::host::{OpenHumanRunContext, SessionTurnSidecar};
use crate::agent::tinyagents::observability::SubagentScope;
use crate::agent::turn_deadline::TurnDeadline;

/// Pauses the run once its turn deadline's wind-down point has passed.
pub(crate) struct DeadlineWindDownMiddleware {
    handle: SteeringHandle,
    deadline: TurnDeadline,
    /// Latched on the first pause, so `Pause` is sent exactly once.
    fired: Arc<AtomicBool>,
    /// The turn's session sidecar; the pause is recorded there so the driver
    /// can report the turn as wound down rather than completed.
    sidecar: std::sync::OnceLock<Arc<std::sync::Mutex<SessionTurnSidecar>>>,
}

impl DeadlineWindDownMiddleware {
    pub(crate) fn new(handle: SteeringHandle, deadline: TurnDeadline) -> Self {
        Self {
            handle,
            deadline,
            fired: Arc::new(AtomicBool::new(false)),
            sidecar: std::sync::OnceLock::new(),
        }
    }

    /// Record the pause on `sidecar` (the turn's run-context sidecar). Set
    /// once, before the run starts.
    pub(crate) fn set_sidecar(&self, sidecar: Arc<std::sync::Mutex<SessionTurnSidecar>>) {
        let _ = self.sidecar.set(sidecar);
    }

    /// Whether this middleware has paused the run.
    pub(crate) fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }

    /// Pause the run if the wind-down point has passed as of `now`. Returns
    /// whether this call sent the pause.
    pub(crate) fn maybe_pause(&self, checkpoint: &'static str, now: Instant) -> bool {
        if !self.deadline.should_wind_down(now) || self.fired.swap(true, Ordering::SeqCst) {
            return false;
        }
        tracing::warn!(
            target: "turn_deadline",
            checkpoint,
            elapsed_ms = self.deadline.elapsed_at(now).as_millis() as u64,
            wind_down_ms = self.deadline.wind_down_after().as_millis() as u64,
            backstop_ms = self.deadline.backstop().as_millis() as u64,
            "[turn_deadline] wind-down point passed; pausing the run so the turn closes with an answer before the backstop"
        );
        self.handle.send(SteeringCommand::Pause);
        if let Some(sidecar) = self.sidecar.get() {
            sidecar
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .wind_down = true;
            tracing::debug!(
                target: "turn_deadline",
                "[turn_deadline] wind-down recorded on the turn sidecar; the turn will be reported as stopped"
            );
        }
        true
    }
}

#[async_trait]
impl<State, Ctx> Middleware<State, Ctx> for DeadlineWindDownMiddleware
where
    State: Send + Sync,
    Ctx: Send + Sync,
{
    fn name(&self) -> &str {
        "openhuman.deadline_wind_down"
    }

    async fn after_model(
        &self,
        _ctx: &mut RunContext<Ctx>,
        _state: &State,
        _response: &mut ModelResponse,
    ) -> tinyagents_harness::Result<()> {
        self.maybe_pause("after_model", Instant::now());
        Ok(())
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<Ctx>,
        _state: &State,
        _invocation: &ToolInvocationIdentity,
        _result: &mut ToolResult,
    ) -> tinyagents_harness::Result<()> {
        self.maybe_pause("after_tool", Instant::now());
        Ok(())
    }
}

/// Install the wind-down for a run that carries a turn deadline, and clamp the
/// harness wall-clock ceiling to its hard stop.
///
/// Children and runs without a deadline are left untouched. A synchronous
/// child is bounded by its parent's tool-call budget, which the clamp already
/// ends at the hard stop. Detached children must not inherit the deadline of
/// the turn that spawned them.
pub(super) fn install(
    harness: &mut AgentHarness<(), OpenHumanRunContext>,
    handle: &Option<SteeringHandle>,
    run_context: &OpenHumanRunContext,
    subagent_scope: &Option<SubagentScope>,
) -> Option<Arc<DeadlineWindDownMiddleware>> {
    let middleware = install_for(
        harness,
        handle.as_ref(),
        run_context.turn_deadline,
        subagent_scope.is_some(),
    )?;
    // `install_for` hands back the instance it registered; record the pause
    // on the turn's sidecar through that same shared state.
    middleware.set_sidecar(run_context.session_sidecar.clone());
    Some(middleware)
}

/// [`install`] over plain values, for tests.
pub(super) fn install_for(
    harness: &mut AgentHarness<(), OpenHumanRunContext>,
    handle: Option<&SteeringHandle>,
    deadline: Option<TurnDeadline>,
    is_subagent: bool,
) -> Option<Arc<DeadlineWindDownMiddleware>> {
    let deadline = deadline.filter(|_| !is_subagent)?;
    let now = Instant::now();
    let mut policy = harness.policy().clone();
    let configured = policy.limits.max_wall_clock_ms;
    policy.limits.max_wall_clock_ms = deadline.clamp_wall_clock_ms(configured, now);
    tracing::debug!(
        target: "turn_deadline",
        configured_ms = ?configured,
        clamped_ms = ?policy.limits.max_wall_clock_ms,
        elapsed_ms = deadline.elapsed_at(now).as_millis() as u64,
        wind_down_ms = deadline.wind_down_after().as_millis() as u64,
        hard_stop_ms = deadline.hard_stop_after().as_millis() as u64,
        backstop_ms = deadline.backstop().as_millis() as u64,
        "[turn_deadline] clamped harness wall clock to the turn's hard stop"
    );
    harness.with_policy(policy);
    let handle = handle?;
    let middleware = Arc::new(DeadlineWindDownMiddleware::new(handle.clone(), deadline));
    harness.push_middleware(middleware.clone());
    Some(middleware)
}

#[cfg(test)]
#[path = "deadline_wind_down_tests.rs"]
mod tests;
