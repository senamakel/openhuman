//! Telling a repeat loop apart from a real blocker when the failure breaker
//! stops a run.
//!
//! The repeat guard (`RepeatProgressMiddleware`) answers a call that would
//! only return a result the model already holds with a refusal, and the call
//! never runs. A refused call is neither progress nor a failure, so it leaves
//! the classified ledger untouched. When a model keeps re-issuing one batch
//! of reads, one of them through a mistyped path, the guard refuses the
//! repeated reads and the mistyped read fails again with nothing in between
//! to clear its `not_found` count: the ledger stops the turn, and the user is
//! told "something this needs could not be found" when the turn was stuck
//! repeating itself. [`RepeatLoopWitness`] records what is needed to say so
//! instead.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Most failed-call signatures remembered in one run.
const MAX_FAILED_CALLS: usize = 256;

/// Classified-failure classes whose stop is relabelled when it comes from a
/// repeat loop: wrong calls the model could have corrected or dropped. A
/// security or credential refusal, an uncertain side effect, an unavailable
/// service or a transient failure keeps its own wording.
const CORRECTABLE_CLASSES: &[&str] = &[
    "not_found",
    "missing_window",
    "missing_app",
    "validation",
    "invalid_arguments",
    "task_failed",
];

/// Whether a classified stop of `class` may be explained by a repeat loop.
pub(super) fn is_correctable_class(class: &str) -> bool {
    CORRECTABLE_CLASSES.contains(&class)
}

/// What the failure breaker has seen of the repeat guard and of repeated
/// failing calls in the current run.
#[derive(Default)]
pub(super) struct RepeatLoopWitness {
    /// `tool\u{1f}argument-fingerprint\u{1f}outcome-hash` of every failure so
    /// far.
    failed_calls: Mutex<HashSet<String>>,
    /// Calls the repeat guard refused since the last successful call.
    refusals_since_progress: AtomicUsize,
}

impl RepeatLoopWitness {
    /// A call ran and succeeded: the run is no longer only repeating itself.
    pub(super) fn record_success(&self) {
        self.refusals_since_progress.store(0, Ordering::SeqCst);
    }

    /// The repeat guard refused a call. Returns the refusals since the last
    /// success, this one included.
    pub(super) fn record_refusal(&self) -> usize {
        self.refusals_since_progress.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Calls the repeat guard refused since the last success.
    pub(super) fn refusals(&self) -> usize {
        self.refusals_since_progress.load(Ordering::SeqCst)
    }

    /// A call failed with `failure_text`. Returns whether the same tool
    /// already failed with the same arguments and the same outcome earlier
    /// in this run (an identical re-issue). The outcome is compared with its
    /// volatile spans (timestamps, durations, request ids) normalised away; a
    /// retry that fails differently (a task that got further, a different
    /// error) is new evidence, not a repeat.
    pub(super) fn record_failure(
        &self,
        tool: &str,
        arg_fingerprint: &str,
        failure_text: &str,
    ) -> bool {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tinyagents_harness::no_progress::normalize_volatile(failure_text).hash(&mut hasher);
        let key = format!("{tool}\u{1f}{arg_fingerprint}\u{1f}{:x}", hasher.finish());
        let Ok(mut failed) = self.failed_calls.lock() else {
            return false;
        };
        if failed.contains(&key) {
            return true;
        }
        if failed.len() < MAX_FAILED_CALLS {
            failed.insert(key);
        }
        false
    }
}

/// The stop note for a run stopped while it re-issued calls it already had
/// results for. `error_line` is the failing call's scrubbed first error line.
///
/// It names the repeating as the cause (the note maps to the
/// "kept repeating the same step" reason, see
/// `session_host::stop_summary::StopReasonKind::from_stop_note`) and still
/// tells the model which call failed and how.
pub(super) fn repeat_loop_halt_summary(tool: &str, refusals: usize, error_line: &str) -> String {
    format!(
        "Stopping: the run kept re-issuing tool calls it already had results for — the repeat \
         guard refused {refusals} identical successful call(s) — and `{tool}` was re-run with \
         identical arguments and failed the same way again. Re-running steps adds no new \
         information. Last error:\n{error_line}\n\nUse the results already in the conversation, \
         correct the failing call, or report back instead of repeating it."
    )
}

#[cfg(test)]
#[path = "repeat_loop_stop_tests.rs"]
mod tests;
