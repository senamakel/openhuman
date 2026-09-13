//! [`RepeatProgressMiddleware`]: host adapter for the crate successful-repeat
//! tracker — halts identical-output / identical-call loops that succeed but
//! make no progress (#4088 / #4095).

use async_trait::async_trait;

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyagents_harness::no_progress::{SuccessfulRepeat, SuccessfulRepeatTracker};
use tinyagents_harness::steering::{SteeringCommand, SteeringHandle};
use tinyagents_harness::tool::ToolResult as TaToolResult;
use tinyinference::message::ContentBlock;
use tinyinference::model::ModelResponse;

use super::loop_guards::is_repeat_call_exempt;

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
///
/// Polling/wait tools ([`is_repeat_call_exempt`]) are exempt from both: their
/// contract is to be re-invoked identically, so an all-poll batch resets the
/// streaks instead of recording. On a trip it writes the legacy root-cause
/// summary into the shared [`HaltSummarySlot`](crate::agent::tinyagents::HaltSummarySlot) and pauses
/// the run through the shared steering handle — the same halt mechanism as the
/// repeated-failure breaker.
pub(crate) struct RepeatProgressMiddleware {
    handle: SteeringHandle,
    halt_summary: crate::agent::tinyagents::HaltSummarySlot,
    tracker: SuccessfulRepeatTracker,
    /// Batch bookkeeping bridging `after_model` → `after_tool` for the call guard.
    pending: std::sync::Mutex<Option<PendingCallBatch>>,
}

impl RepeatProgressMiddleware {
    pub(crate) fn new(
        handle: SteeringHandle,
        halt_summary: crate::agent::tinyagents::HaltSummarySlot,
    ) -> Self {
        Self {
            handle,
            halt_summary,
            tracker: SuccessfulRepeatTracker::default(),
            pending: std::sync::Mutex::new(None),
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

        // Stage output with the crate tracker. Its halt verdict is intentionally
        // deferred until the matching tool batch is confirmed successful.
        let _ = self.tracker.record_output(&output_sig, all_exempt);

        // Stage the batch for the repeat-CALL guard, evaluated once every result
        // is back (gated on success) in `after_tool`.
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(PendingCallBatch {
                call_sig,
                remaining: tool_calls.len(),
                all_ok: true,
                exempt: all_exempt,
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
        // Fold this result into the pending batch; only act once the batch is
        // complete so the call guard sees whole-batch success.
        let completed = {
            let Ok(mut pending) = self.pending.lock() else {
                return Ok(());
            };
            let Some(batch) = pending.as_mut() else {
                return Ok(());
            };
            if result.error.is_some() {
                batch.all_ok = false;
            }
            batch.remaining = batch.remaining.saturating_sub(1);
            if batch.remaining == 0 {
                pending.take()
            } else {
                None
            }
        };
        let Some(batch) = completed else {
            return Ok(());
        };

        if let SuccessfulRepeat::Halt(summary) =
            self.tracker
                .record_call_batch(&batch.call_sig, batch.all_ok, batch.exempt)
        {
            tracing::warn!("[tinyagents::mw] crate successful-repeat tracker halted the run");
            self.halt(summary);
        }
        Ok(())
    }
}
