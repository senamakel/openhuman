//! Shared bridge state types and the model-call cap pauser.
//!
//! These types are shared between the `OpenhumanEventBridge` (in
//! [`super::event_bridge`]) and the model adapter, which is why they live in
//! their own submodule rather than beside the bridge itself.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tinyagents_harness::events::{AgentEvent, EventListener, EventRecord};
use tinyagents_harness::steering::{SteeringCommand, SteeringHandle};

use crate::agent::harness::turn_dispatch_guard::TurnDispatchState;

/// Attribution for child (sub-agent) progress. When present, the bridge routes
/// events to the `Subagent*` [`AgentProgress`](crate::agent::progress::AgentProgress)
/// variants (so the parent thread can nest child activity under a live
/// subagent row) instead of the top-level ones. Absent = a parent/top-level
/// turn.
#[derive(Clone)]
pub struct SubagentScope {
    pub agent_id: String,
    pub task_id: String,
    pub extended_policy: bool,
}

/// A shared 1-based model-call (iteration) cursor. The bridge advances it on
/// each `ModelStarted` event; the model adapter reads it to attribute the
/// tool-argument deltas it still forwards out-of-band.
pub(crate) type IterationCursor = Arc<AtomicU32>;

/// A shared `call_id → tool_name` map. The model adapter's `ThinkingForwarder`
/// writes it when a tool call *starts* (the crate `ToolDelta` has no `tool_name`
/// field, so the start-event/name half of the tool-arg contract can't ride the
/// crate stream and stays on the out-of-band forwarder path — see
/// [`super::model::ThinkingForwarder`]). The bridge reads it to label the
/// incremental tool-argument fragments it now projects off the crate stream
/// (`MessageDelta.tool_call`), preserving the UI's `ToolCallArgsDelta`
/// `tool_name` contract without the forwarder emitting those fragments itself.
pub(crate) type ToolNameMap = Arc<Mutex<std::collections::HashMap<String, String>>>;

/// Shared `call_id → (success, classified failure, elapsed_ms, output_chars)`
/// side-channel. The crate's `AgentEvent::ToolCompleted` carries only `call_id`
/// + `tool_name` (no success/error, duration, or output size), so
///
/// `ToolOutcomeCaptureMiddleware::after_tool` — which does see the `ToolResult`
/// (including the executor-measured `elapsed_ms` and the rendered content) —
/// classifies each outcome and writes it here; the bridge reads it when
/// projecting the live `ToolCallCompleted` event, so a failed tool surfaces real
/// `success: false` + a user-facing `failure`, and a completed tool surfaces its
/// real duration + output size instead of `0`/`0` (#4467, item 4). Absent entry
/// (event projected before the middleware ran) falls back to `(true, None, 0, 0)`.
pub(crate) type ToolFailureMap = Arc<
    Mutex<
        std::collections::HashMap<
            String,
            (
                bool,
                Option<crate::tools::status::ClassifiedFailure>,
                u64,
                usize,
            ),
        >,
    >,
>;

/// Shared FIFO carry of the per-call provider [`UsageInfo`](crate::inference::provider::UsageInfo)
/// the model adapter observed, drained by the bridge when it records that
/// call's usage. The crate `Usage` the harness surfaces on
/// `AgentEvent::UsageRecorded` carries only token counts, so the
/// backend-charged USD, the model's context window, and the
/// cache-creation/reasoning token breakdown have no crate home — the model
/// adapter pushes the full provider `UsageInfo` here (one push per provider
/// response) and the bridge pops it (one pop per recorded model call, after the
/// duplicate-usage dedupe guard) to restore charged-USD precedence and the full
/// accounting (#4467, item 1). A pop that finds nothing (a fallback-route call
/// that did not push, or an out-of-band usage event) degrades gracefully to a
/// catalogue estimate.
pub(crate) type ProviderUsageCarry =
    Arc<Mutex<std::collections::VecDeque<crate::inference::provider::UsageInfo>>>;

/// An [`EventListener`] that pauses the run once `cap` model calls have
/// completed, so the loop stops gracefully at the iteration budget (returning
/// the partial transcript) instead of erroring with `LimitExceeded`. The harness
/// checks pending steering at the top of each turn *before* the model-call limit
/// check, so a `Pause` sent here short-circuits the loop cleanly. The caller then
/// inspects the run's finish reason to decide whether to summarize a checkpoint
/// — the tinyagents analogue of the legacy cap checkpoint seam.
///
/// Only the capped run's **own** model calls count. The sink is shared with
/// every in-process nested run (the payload summarizer runs its child on it via
/// `SubAgent::invoke_with_events`), and those children emit `ModelCompleted`
/// too. Counting them paused the turn after `cap - N` of its own calls while
/// `hit_cap` — which reads `run.model_calls`, the run's own count — stayed
/// `false`, so the turn ended on the no-final-text path instead of the cap
/// checkpoint (#6280).
pub(crate) struct CapPauser {
    handle: SteeringHandle,
    cap: u32,
    /// Id of the run being capped. The crate mints that run's model call ids as
    /// `{run_id}-model-{n}` (`agent_loop/run_loop.rs`), while a nested child
    /// runs under its own `{name}-d{depth}-{seq}` id, so the call id alone
    /// attributes a completion to its run at any nesting depth, with no
    /// dependence on start/finish events a failed child never emits.
    run_id: String,
    completed: AtomicU32,
    /// The current turn's dispatch guard, when this run is a turn (rather than
    /// a CLI/direct invocation). Recording the pause here is what makes it
    /// *binding* on new sub-agent dispatch instead of merely advisory — see
    /// [`crate::agent::harness::turn_dispatch_guard`] and #5804.
    dispatch_guard: Option<Arc<TurnDispatchState>>,
}

impl CapPauser {
    /// Pause `handle` once `cap` of run `run_id`'s own model calls complete,
    /// recording the pause on `dispatch_guard` when the run is executing inside
    /// a turn scope.
    pub(crate) fn new(
        handle: SteeringHandle,
        cap: usize,
        run_id: impl Into<String>,
        dispatch_guard: Option<Arc<TurnDispatchState>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            handle,
            cap: cap as u32,
            run_id: run_id.into(),
            completed: AtomicU32::new(0),
            dispatch_guard,
        })
    }

    /// `true` when `call_id` is one of the capped run's own model calls
    /// (`{run_id}-model-{n}`), not a nested run's.
    fn is_own_model_call(&self, call_id: &str) -> bool {
        call_id
            .strip_prefix(self.run_id.as_str())
            .and_then(|rest| rest.strip_prefix("-model-"))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    }
}

impl EventListener for CapPauser {
    fn on_event(&self, record: &EventRecord) {
        let AgentEvent::ModelCompleted { call_id, .. } = &record.event else {
            return;
        };
        if !self.is_own_model_call(call_id.as_str()) {
            tracing::debug!(
                call_id = call_id.as_str(),
                run_id = %self.run_id,
                "[tinyagents] model-call cap: ignoring a nested run's model completion"
            );
            return;
        }
        let n = self.completed.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= self.cap {
            tracing::info!(
                completed = n,
                cap = self.cap,
                "[tinyagents] model-call cap reached — requesting graceful pause"
            );
            // Record BEFORE sending the advisory command. The crate drains
            // its event queue synchronously, notifying listeners in
            // insertion order on the emitting task
            // (`vendor/tinyagents/src/harness/events/mod.rs:163-195`), so
            // this store happens-before any tool call the loop dispatches
            // afterwards. That ordering is the whole fix: the pause stops
            // being something a dispatch can race and becomes something a
            // dispatch must observe.
            if let Some(guard) = self.dispatch_guard.as_ref() {
                guard.record_pause_requested(u64::from(n), u64::from(self.cap));
            }
            self.handle.send(SteeringCommand::Pause);
        }
    }
}

#[cfg(test)]
#[path = "cap_pauser_tests.rs"]
mod tests;
