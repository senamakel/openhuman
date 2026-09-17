//! Map a failed `tinyagents` harness run onto the typed OpenHuman error the
//! caller expects, for [`run_turn_via_tinyagents_shared`](super::run_turn_via_tinyagents_shared).

use crate::agent::tinyagents::journal::TurnJournal;
use crate::agent::tinyagents::model::ModelErrorSlot;
use crate::agent::tinyagents::turn_models::tinyagents_depth_error;

/// Map the harness's run failure `e` onto the typed error the turn caller
/// expects, stamping the durable journal's terminal failed status first
/// (best-effort, non-fatal).
///
/// #4457 (defect B): the run's *own* definitively-non-provider failure kinds
/// are mapped FIRST, before consulting `error_slot`. The slot preserves the
/// last provider error the model adapter saw — but the adapter now clears it
/// on every successful call (see `native model adapter::chat`/`stream`), so a
/// stale slot should not exist here. Ordering the cap/depth mappings ahead of
/// the slot is defense-in-depth: a run that failed on the model-call cap or a
/// spawn-depth limit is not a provider error, so it must surface as
/// `MaxIterationsExceeded` / the depth error rather than a leftover provider
/// error (wrong classification, wrong Sentry suppression, wrong user
/// message).
pub(super) async fn map_turn_run_error(
    e: tinyagents_harness::TinyAgentsError,
    model: &str,
    max_iterations: usize,
    error_slot: &ModelErrorSlot,
    turn_journal: Option<&TurnJournal>,
) -> anyhow::Error {
    // Durable journal: stamp the terminal failed status (best-effort,
    // non-fatal) before unwinding through the typed-error mapping below.
    if let Some(journal) = turn_journal {
        journal.finish_failed(&e.to_string()).await;
    }
    // The model-call cap (when not pausing gracefully — the channel/CLI
    // path) maps to the typed `AgentError::MaxIterationsExceeded` so
    // callers downcast it (Sentry skip) and render the canonical
    // "Agent exceeded maximum tool iterations" message, matching the
    // legacy `ErrorCheckpoint`.
    if let tinyagents_harness::TinyAgentsError::LimitExceeded(msg) = &e {
        if msg.contains("model call") {
            tracing::debug!(
                model,
                "[tinyagents] run hit the model-call cap; mapping to MaxIterationsExceeded (not consulting error_slot) — #4457 defect B"
            );
            return anyhow::Error::new(crate::agent::error::AgentError::MaxIterationsExceeded {
                max: max_iterations,
            });
        }
    }
    if let Some(depth_err) = tinyagents_depth_error(&e) {
        return anyhow::Error::new(depth_err);
    }
    // Otherwise prefer the original typed provider error (preserves
    // `AgentError` downcasts the caller relies on) over the harness's
    // string wrap — this is where a genuine model/provider failure that
    // halted the run is re-surfaced with its real classification.
    // #4469 item 3: `into_inner` recovers a poisoned slot so a panic in
    // one run can't cascade into a second panic here that would mask the
    // original typed provider error.
    if let Some(original) = error_slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        tracing::debug!(
            model,
            "[tinyagents] re-surfacing typed provider error from error_slot as the run failure — #4457 defect B"
        );
        return original;
    }
    anyhow::anyhow!("tinyagents harness run failed: {e}")
}
