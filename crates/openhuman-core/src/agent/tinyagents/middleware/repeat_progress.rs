//! [`RepeatProgressMiddleware`]: host adapter for the crate successful-repeat
//! tracker — halts identical-output / identical-call loops that succeed but
//! make no progress (#4088 / #4095), including loops whose repeats are not
//! back to back (#6275).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyagents_harness::no_progress::{
    fingerprint_arguments, SuccessfulRepeat, SuccessfulRepeatTracker,
};
use tinyagents_harness::steering::{SteeringCommand, SteeringHandle};
use tinyagents_harness::tool::ToolResult as TaToolResult;
use tinyinference::message::{ContentBlock, Message};
use tinyinference::model::{ModelRequest, ModelResponse};

use super::loop_guards::is_repeat_call_exempt;
use crate::agent::context::CLEARED_PLACEHOLDER;

/// Extract the assistant's visible text (concatenated [`ContentBlock::Text`]
/// blocks) from a model response message, for the repeat-output signature.
fn assistant_visible_text(message: &tinyinference::message::AssistantMessage) -> String {
    let mut out = String::new();
    for block in &message.content {
        if let ContentBlock::Text(t) = block {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    out
}

/// Per-batch state the repeat-CALL guard needs but can only fully evaluate once
/// every tool result in the assistant's batch has come back: the canonical
/// `(tool, args)` signature captured at `after_model`, plus the running
/// success/remaining accounting folded in at each `after_tool`.
#[derive(Default)]
struct PendingCallBatch {
    /// Canonical `(tool, args)` signature of the batch, from `after_model`.
    call_sig: String,
    /// Tool results still outstanding for this batch.
    remaining: usize,
    /// `true` while every result so far in the batch has succeeded.
    all_ok: bool,
    /// `true` when every call in the batch is a polling/wait exemption.
    exempt: bool,
    /// `call_id` → per-call `(tool, argument fingerprint)` signature for the
    /// recurrence ledger. Polling/wait calls are left out.
    call_sigs: HashMap<String, String>,
    /// `true` once a result in this batch has already halted the run, so the
    /// batch does not pause it a second time.
    halted: bool,
}

/// Tracker state shared between [`RepeatProgressMiddleware`] and its
/// [`RepeatEvictionObserver`].
#[derive(Default)]
struct RepeatState {
    tracker: SuccessfulRepeatTracker,
    /// `call_id`s of the results fed to the recurrence ledger since its last reset.
    recorded: Mutex<HashSet<String>>,
    /// Recorded results still verbatim in the current request before any
    /// reduction step ran; compared against the final request by the observer.
    visible_before_reduction: Mutex<HashSet<String>>,
}

/// The `ids` whose tool result is still in `request` with its body intact.
fn visible_tool_results(request: &ModelRequest, ids: &HashSet<String>) -> HashSet<String> {
    request
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Tool(tool)
                if ids.contains(&tool.tool_call_id) && message.text() != CLEARED_PLACEHOLDER =>
            {
                Some(tool.tool_call_id.clone())
            }
            _ => None,
        })
        .collect()
}

/// Host adapter for the crate's successful-repeat tracker (#4088 / #4095).
/// [`SuccessfulRepeatTracker`] owns the generic streak accounting; this adapter
/// builds canonical OpenHuman tool signatures, applies the product polling-tool
/// exemption, and maps a crate halt verdict into the shared halt summary and
/// steering pause:
///
/// - **Repeat-output** (`after_model`, checked before the tools run): halts when
///   the assistant's visible text + tool-call `(name, args)` batch is byte
///   identical [`DEFAULT_REPEAT_OUTPUT_THRESHOLD`] iterations in a row.
/// - **Repeat-call** (evaluated once the batch's tool results are all back, gated
///   on every call succeeding): halts when the `(tool, args)` batch alone repeats
///   [`DEFAULT_REPEAT_CALL_THRESHOLD`] times — catching successful no-op loops
///   that vary only their narration.
/// - **Recurrence** (each successful `after_tool`, #6275): halts when one call
///   returns the identical result [`DEFAULT_REPEAT_CALL_THRESHOLD`] times in the
///   run, adjacent or not — catching a model cycling A, B, A, B through steps
///   whose results it already has. The result is the content after the earlier
///   `after_tool` layers (caps, summarizer), i.e. what the model saw, so a
///   re-read whose output changed does not count. A result that compaction
///   later evicts stops counting; see [`RepeatEvictionObserver`].
///
/// Polling/wait tools ([`is_repeat_call_exempt`]) are exempt from all three:
/// their contract is to be re-invoked identically, so an all-poll batch resets
/// the streaks instead of recording. On a trip it writes the legacy root-cause
/// summary into the shared [`HaltSummarySlot`](crate::agent::tinyagents::HaltSummarySlot) and pauses
/// the run through the shared steering handle — the same halt mechanism as the
/// repeated-failure breaker.
///
/// [`DEFAULT_REPEAT_OUTPUT_THRESHOLD`]: tinyagents_harness::no_progress::DEFAULT_REPEAT_OUTPUT_THRESHOLD
/// [`DEFAULT_REPEAT_CALL_THRESHOLD`]: tinyagents_harness::no_progress::DEFAULT_REPEAT_CALL_THRESHOLD
pub(crate) struct RepeatProgressMiddleware {
    handle: SteeringHandle,
    halt_summary: crate::agent::tinyagents::HaltSummarySlot,
    state: Arc<RepeatState>,
    /// Batch bookkeeping bridging `after_model` → `after_tool` for the call guard.
    pending: Mutex<Option<PendingCallBatch>>,
}

impl RepeatProgressMiddleware {
    pub(crate) fn new(
        handle: SteeringHandle,
        halt_summary: crate::agent::tinyagents::HaltSummarySlot,
    ) -> Self {
        Self {
            handle,
            halt_summary,
            state: Arc::new(RepeatState::default()),
            pending: Mutex::new(None),
        }
    }

    /// The companion that resets the recurrence ledger when a recorded result is
    /// evicted from context. It must be registered after every reduction
    /// middleware, and this guard before them.
    pub(crate) fn eviction_observer(&self) -> RepeatEvictionObserver {
        RepeatEvictionObserver {
            state: Arc::clone(&self.state),
        }
    }

    /// Latch a root-cause halt: record the summary the turn surfaces instead of an
    /// empty/last-model reply, and pause at the top of the next iteration (before
    /// the next model call), matching the repeated-failure breaker's halt path.
    fn halt(&self, summary: String) {
        if let Ok(mut slot) = self.halt_summary.lock() {
            *slot = Some(summary);
        }
        self.handle.send(SteeringCommand::Pause);
    }
}

#[async_trait]
impl Middleware<()> for RepeatProgressMiddleware {
    fn name(&self) -> &str {
        "repeat_progress"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        // Registered ahead of the reduction steps, so this is the request as the
        // loop built it. The observer compares the same ids after they ran.
        let visible = match self.state.recorded.lock() {
            Ok(recorded) if !recorded.is_empty() => visible_tool_results(request, &recorded),
            _ => HashSet::new(),
        };
        if let Ok(mut slot) = self.state.visible_before_reduction.lock() {
            *slot = visible;
        }
        Ok(())
    }

    async fn after_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        response: &mut ModelResponse,
    ) -> TaResult<()> {
        let tool_calls = &response.message.tool_calls;
        if tool_calls.is_empty() {
            // A final answer (no tool calls) ends the loop; nothing to guard, and
            // there is no batch to track for the call guard.
            if let Ok(mut pending) = self.pending.lock() {
                *pending = None;
            }
            return Ok(());
        }

        // Polling/wait tools are contractually re-invoked with identical args +
        // narration each timeout while the work is still running, so an all-poll
        // batch is legitimate progress, not a no-progress repeat.
        let all_exempt = tool_calls.iter().all(|c| is_repeat_call_exempt(&c.name));

        // Canonical `(tool, args)` batch signature (call guard) and the broader
        // narration+call signature (output guard). Both fold each call in order
        // with a `\u{1}` separator, matching the legacy signatures.
        let mut call_sig = String::new();
        for call in tool_calls {
            call_sig.push('\u{1}');
            call_sig.push_str(&call.name);
            call_sig.push('\u{1}');
            call_sig.push_str(&call.arguments.to_string());
        }
        let output_sig = format!(
            "{}{}",
            assistant_visible_text(&response.message).trim(),
            call_sig
        );
        // Per-call signatures for the recurrence ledger, keyed by `call_id` so
        // each result can be matched to the arguments that produced it.
        let call_sigs = tool_calls
            .iter()
            .filter(|call| !is_repeat_call_exempt(&call.name))
            .map(|call| {
                (
                    call.id.clone(),
                    format!(
                        "{}\u{1}{}",
                        call.name,
                        fingerprint_arguments(&call.arguments)
                    ),
                )
            })
            .collect();

        // Stage output with the crate tracker. Its halt verdict is intentionally
        // deferred until the matching tool batch is confirmed successful.
        let _ = self.state.tracker.record_output(&output_sig, all_exempt);

        // Stage the batch for the repeat-CALL guard, evaluated once every result
        // is back (gated on success) in `after_tool`.
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(PendingCallBatch {
                call_sig,
                remaining: tool_calls.len(),
                all_ok: true,
                exempt: all_exempt,
                call_sigs,
                halted: false,
            });
        }
        Ok(())
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        result: &mut TaToolResult,
    ) -> TaResult<()> {
        // Fold this result into the pending batch; the call guard only acts once
        // the batch is complete so it sees whole-batch success.
        let (already_halted, recurrence, completed) = {
            let Ok(mut pending) = self.pending.lock() else {
                return Ok(());
            };
            let Some(batch) = pending.as_mut() else {
                return Ok(());
            };
            let already_halted = batch.halted;
            let mut recurrence = SuccessfulRepeat::Continue;
            if result.error.is_some() {
                batch.all_ok = false;
            } else if let Some(sig) = batch.call_sigs.get(&result.call_id) {
                recurrence = self.state.tracker.record_call_outcome(sig, &result.content);
                if let Ok(mut recorded) = self.state.recorded.lock() {
                    recorded.insert(result.call_id.clone());
                }
            }
            if matches!(recurrence, SuccessfulRepeat::Halt(_)) {
                batch.halted = true;
            }
            batch.remaining = batch.remaining.saturating_sub(1);
            let completed = if batch.remaining == 0 {
                pending.take()
            } else {
                None
            };
            (already_halted, recurrence, completed)
        };
        if already_halted {
            // An earlier result in this batch paused the run; keep the streak
            // accounting current without pausing again.
            if let Some(batch) = completed {
                let _ = self.state.tracker.record_call_batch(
                    &batch.call_sig,
                    batch.all_ok,
                    batch.exempt,
                );
            }
            return Ok(());
        }

        let batch_verdict = completed.map(|batch| {
            self.state
                .tracker
                .record_call_batch(&batch.call_sig, batch.all_ok, batch.exempt)
        });
        // When both fire on the same result, the batch summary wins: it is the
        // more specific description of an adjacent repeat.
        let summary = match (batch_verdict, recurrence) {
            (Some(SuccessfulRepeat::Halt(summary)), _) => summary,
            (_, SuccessfulRepeat::Halt(summary)) => summary,
            _ => return Ok(()),
        };
        tracing::warn!(
            tool = %result.name,
            "[tinyagents::mw] crate successful-repeat tracker halted the run"
        );
        self.halt(summary);
        Ok(())
    }
}

/// Resets [`RepeatProgressMiddleware`]'s recurrence ledger when context
/// reduction evicts a recorded tool result (#6275).
///
/// Compression, microcompact and trim rewrite only the outgoing request, never
/// the loop's transcript, so the eviction is visible only inside the
/// `before_model` chain. The guard snapshots which recorded results are intact
/// before those steps; this observer, registered after them, checks the same
/// ids in the final request. A result that was blanked or dropped is no longer
/// in front of the model, so re-reading it is not a repeat the model can see,
/// and the tracker restarts. Keyed on what disappeared rather than on which
/// middleware removed it, so any reduction step is covered, and a dialect that
/// never carries tool results as tool messages is never snapshotted.
pub(crate) struct RepeatEvictionObserver {
    state: Arc<RepeatState>,
}

#[async_trait]
impl Middleware<()> for RepeatEvictionObserver {
    fn name(&self) -> &str {
        "repeat_progress_eviction"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let before = match self.state.visible_before_reduction.lock() {
            Ok(mut slot) => std::mem::take(&mut *slot),
            Err(_) => return Ok(()),
        };
        if before.is_empty() {
            return Ok(());
        }
        let evicted = before.len() - visible_tool_results(request, &before).len();
        if evicted == 0 {
            return Ok(());
        }
        tracing::debug!(
            evicted,
            "[tinyagents::mw] repeat-progress ledger reset: recorded tool results left the context"
        );
        self.state.tracker.reset();
        if let Ok(mut recorded) = self.state.recorded.lock() {
            recorded.clear();
        }
        Ok(())
    }
}
