//! [`StopLatch`]: how the failure breaker records why it stopped a run.
//!
//! The stop-note slot ([`HaltSummarySlot`]) is shared by the failure breaker,
//! the repeat guard (`RepeatProgressMiddleware`) and the stop hooks, and the
//! user-facing stop summary is derived from whatever note it holds when the
//! run ends (`session_host::stop_summary`). A stop only takes effect at the
//! top of the next iteration, so every remaining result of the batch still
//! reaches the breakers after one of them stopped the run. The first breaker
//! to stop the run is its cause, so no breaker replaces a note already there
//! (the repeat guard follows the same contract in `tinyagents`). The failure
//! breaker used to overwrite it: the repeat guard stopping a run while
//! admitting a batch of re-reads, then a mistyped path in that batch failing
//! again, was summarised as "something this needs could not be found".
//!
//! The latch also relabels a classified stop that a repeat loop produced (see
//! [`super::repeat_loop_stop`]), and takes the relabel back when the rest of
//! the batch shows progress.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tinyagents_harness::steering::{SteeringCommand, SteeringHandle};

use super::repeat_loop_stop::{is_correctable_class, repeat_loop_halt_summary, RepeatLoopWitness};
use crate::agent::tinyagents::HaltSummarySlot;

/// The failure breaker's side of the shared stop-note slot.
pub(super) struct StopLatch {
    handle: SteeringHandle,
    slot: HaltSummarySlot,
    /// The note this breaker latched in the current run.
    own: Mutex<Option<String>>,
    /// Repeat-guard refusals and repeated failing calls in the current run.
    witness: RepeatLoopWitness,
    /// `(tool, error line)` of a latched classified stop on an identical
    /// re-issue that no refusal has explained yet. A refusal later in the
    /// same batch relabels the stop as a repeat loop.
    candidate: Mutex<Option<(String, String)>>,
    /// The classified note a repeat-loop relabel replaced. A success later in
    /// the batch shows progress and puts it back.
    relabelled_from: Mutex<Option<String>>,
    /// Whether a call in the current model response's tool batch succeeded.
    /// A batch that produced anything new is never relabelled as a repeat
    /// loop, whatever order its results fold back in.
    batch_progress: AtomicBool,
}

impl StopLatch {
    /// A latch that pauses runs through `handle` and records notes in `slot`.
    pub(super) fn new(handle: SteeringHandle, slot: HaltSummarySlot) -> Self {
        Self {
            handle,
            slot,
            own: Mutex::default(),
            witness: RepeatLoopWitness::default(),
            candidate: Mutex::default(),
            relabelled_from: Mutex::default(),
            batch_progress: AtomicBool::new(false),
        }
    }

    /// What has been seen of the repeat guard and of repeated failures.
    pub(super) fn witness(&self) -> &RepeatLoopWitness {
        &self.witness
    }

    /// Stop the run, recording `summary` as its stop note unless another
    /// breaker already stopped it. The pause is sent either way; it is
    /// idempotent. Returns whether `summary` became the stop note.
    pub(super) fn halt(&self, summary: String) -> bool {
        let latched = match self.slot.lock() {
            Ok(mut slot) => match slot.as_deref() {
                Some(existing) => {
                    tracing::debug!(
                        existing = %crate::util::truncate_with_ellipsis(existing, 80),
                        "[tinyagents::mw] run already stopped by an earlier breaker; keeping its stop note"
                    );
                    false
                }
                None => {
                    *slot = Some(summary.clone());
                    if let Ok(mut own) = self.own.lock() {
                        *own = Some(summary);
                    }
                    true
                }
            },
            Err(_) => false,
        };
        self.handle.send(SteeringCommand::Pause);
        latched
    }

    /// Stop the run for an exhausted classified-failure budget.
    ///
    /// When the failing call is an identical re-issue (`repeated`) of a
    /// correctable class and the repeat guard has refused calls since the
    /// last success, the run was looping, and the note says so. With no
    /// refusal yet, a refusal later in the batch can still relabel it
    /// ([`Self::record_refusal`]).
    pub(super) fn halt_classified(
        &self,
        summary: String,
        class: &str,
        tool: &str,
        repeated: bool,
        error_line: impl FnOnce() -> String,
    ) {
        if !(repeated && is_correctable_class(class)) {
            self.halt(summary);
            return;
        }
        let error_line = error_line();
        // Only the stop that owns the note may be relabelled: a second stop
        // in the batch must not lend it its tool and error.
        if !self.halt(summary) {
            return;
        }
        match self.witness.refusals() {
            0 => {
                if let Ok(mut candidate) = self.candidate.lock() {
                    *candidate = Some((tool.to_owned(), error_line));
                }
            }
            refusals => self.relabel(repeat_loop_halt_summary(tool, refusals, &error_line)),
        }
    }

    /// The repeat guard refused a call. Relabels this breaker's classified
    /// stop as a repeat loop if it is waiting for that evidence.
    pub(super) fn record_refusal(&self) {
        let refusals = self.witness.record_refusal();
        let Some((tool, error_line)) = self.candidate.lock().ok().and_then(|mut c| c.take()) else {
            return;
        };
        self.relabel(repeat_loop_halt_summary(&tool, refusals, &error_line));
    }

    /// A call ran and succeeded. The run made progress, so neither the
    /// refusals around it nor a stop latched earlier in its batch can be
    /// explained as a repeat loop: a pending relabel is dropped and a relabel
    /// already made is taken back.
    pub(super) fn record_success(&self) {
        self.batch_progress.store(true, Ordering::SeqCst);
        self.witness.record_success();
        if let Ok(mut candidate) = self.candidate.lock() {
            *candidate = None;
        }
        let original = self.relabelled_from.lock().ok().and_then(|mut r| r.take());
        if let Some(original) = original {
            tracing::debug!(
                "[tinyagents::mw] a success followed a repeat-loop stop; restoring the classified stop note"
            );
            self.replace_own_note(original);
        }
    }

    /// A model response asked for a new tool batch: nothing in it has
    /// succeeded yet.
    pub(super) fn start_batch(&self) {
        self.batch_progress.store(false, Ordering::SeqCst);
    }

    /// End of a run: forget its latch state.
    pub(super) fn finish_run(&self) {
        for state in [&self.own, &self.relabelled_from] {
            if let Ok(mut state) = state.lock() {
                *state = None;
            }
        }
        if let Ok(mut candidate) = self.candidate.lock() {
            *candidate = None;
        }
    }

    /// Replace this breaker's latched note with the repeat-loop `summary`,
    /// keeping the classified note so progress can restore it.
    fn relabel(&self, summary: String) {
        if self.batch_progress.load(Ordering::SeqCst) {
            tracing::debug!(
                "[tinyagents::mw] the stop's batch made progress; keeping the classified stop note"
            );
            return;
        }
        if let Some(original) = self.replace_own_note(summary) {
            tracing::debug!(
                "[tinyagents::mw] classified stop came from a repeat loop; relabelled its stop note"
            );
            if let Ok(mut relabelled_from) = self.relabelled_from.lock() {
                relabelled_from.get_or_insert(original);
            }
        }
    }

    /// Swap `summary` in for this breaker's note, if that note is still the
    /// run's stop note. Returns the note it replaced.
    fn replace_own_note(&self, summary: String) -> Option<String> {
        let (Ok(mut slot), Ok(mut own)) = (self.slot.lock(), self.own.lock()) else {
            return None;
        };
        if own.is_none() || *slot != *own {
            return None;
        }
        *slot = Some(summary.clone());
        own.replace(summary)
    }
}
