//! `memory_tree_retry_failed` and the typed failed-job cause it clears
//! (#002 FR-004, FR-011).

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::rpc::RpcOutcome;

/// Response from `memory_tree_retry_failed` (#002 FR-011).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryFailedResponse {
    /// Number of `failed` jobs flipped back to `ready` for retry.
    pub requeued: u64,
}

/// `memory_tree_retry_failed` RPC handler (#002 FR-011). Flips every
/// terminally-`failed` `mem_tree_jobs` row back to `ready` (fresh attempt
/// budget, typed reason cleared) so jobs that failed under a now-fixed config
/// re-run without re-ingesting source data. Backs the "Retry failed" button.
pub async fn retry_failed_rpc(config: &Config) -> Result<RpcOutcome<RetryFailedResponse>, String> {
    // Requeue and wake are one operation at the driver. They were two calls
    // here, which is one call away from a retry that moves rows and then lets
    // them sit until the next scheduled window.
    let requeued = crate::memory::ops::maintenance::retry_failed(config).await?;
    Ok(RpcOutcome::single_log(
        RetryFailedResponse { requeued },
        format!("memory_tree: retry_failed requeued={requeued}"),
    ))
}

/// #002 (FR-004): the typed [`PipelineFailure`] of the most-recently-failed
/// `mem_tree_jobs` row, when it carries a classified `failure_reason` **and that
/// failure is still the pipeline's current blocking cause**. Returns `Ok(None)`
/// when there is no failed job with a typed reason (older failures predating the
/// typed-failure columns, or none at all), or when the failure has been
/// superseded (below). Best-effort: the status panel is a UI convenience, so a
/// DB error degrades to `Ok(None)` rather than failing the whole status RPC.
///
/// # Supersession — why the newest failed row is not automatically the cause
///
/// An unrecoverable failure is terminal by design: it is never retried, so its
/// row sits in `failed` forever with whatever `failure_reason` it died with.
/// Reading that row unconditionally means the panel keeps rendering the *first*
/// diagnosis it ever saw, indefinitely, no matter what the pipeline has done
/// since.
///
/// In production that surfaced as a signed-in user being told "No embeddings
/// credentials found. Log in to OpenHuman" — the remediation for an
/// `auth_missing` batch that had failed **27 days earlier**, while the queue had
/// been completing jobs normally the whole time. The banner was a tombstone, and
/// following it was impossible: the user was already logged in.
///
/// So a failure only counts as the *current* blocking cause when the queue has
/// not settled a job successfully since it. `completed_at_ms` on the newest
/// `done` row is that watermark: if the pipeline has produced output more
/// recently than the failure, the failure describes the past, not the present.
/// The failure is still counted (`failed_unrecoverable` keeps the status at
/// `error` and the "N unrecoverable failure(s) need action" reason), and "Retry
/// failed" is how the user clears it — but the *remediation text*, which tells
/// the user what to go and do right now, is withheld once it stops being true.
///
/// `pub(super)` — shared with [`super::pipeline_status`].
pub(super) async fn latest_failed_job_failure(
    config: &Config,
) -> Result<Option<crate::memory::tree::health::PipelineFailure>, String> {
    // The failure and the success watermark arrive together, as ONE answer.
    // That is not a convenience: asking twice lets a job settle between the
    // two and flip the supersession decision below, which is the race the
    // #5427 review flagged. The driver reads both on one connection; taking
    // `QueueFailure::last_success_ms` from a second call would undo that.
    let binding = crate::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] pipeline_status: driver '{}' does not serve Maintenance; no blocking cause",
            binding.driver_id()
        );
        return Ok(None);
    };
    let reported = maintenance
        .latest_queue_failure()
        .await
        .map_err(|e| format!("latest_failed_job_failure: {e}"))?;
    Ok(reported.as_ref().and_then(blocking_cause))
}

/// The supersession rule, over one failure the driver reported.
///
/// Split from the fetch above so it stays exercisable without a bound driver.
/// The rule is the part with edge cases — an untimestamped failure, a
/// watermark on the same millisecond, a reason this build does not know — and
/// a test that has to stand up a driver to reach it tends not to cover them.
///
/// `pub(super)` — reused verbatim by tests declared directly under `rpc`.
pub(super) fn blocking_cause(
    reported: &crate::memory::api::provider::types::QueueFailure,
) -> Option<crate::memory::tree::health::PipelineFailure> {
    use crate::memory::tree::health::{FailureClass, FailureCode, PipelineFailure};

    let reason = &reported.reason;
    let class = reported.class.clone();
    let failed_at_ms = reported.completed_at_ms;
    let last_success_ms = reported.last_success_ms;

    // Log every supersession branch, not only the withheld one, so the decision
    // is greppable from the logs alone.
    match failed_at_ms {
        Some(failed_at_ms)
            if last_success_ms.is_some_and(|success_ms| success_ms > failed_at_ms) =>
        {
            log::debug!(
                "[memory-tree][rpc] pipeline_status: withholding blocking cause reason={reason} \
                 — the queue has completed a job since it failed (superseded)"
            );
            return None;
        }
        Some(_) => {
            log::debug!(
                "[memory-tree][rpc] pipeline_status: blocking cause is live reason={reason} \
                 — no successful settle since it failed"
            );
        }
        None => {
            log::debug!(
                "[memory-tree][rpc] pipeline_status: blocking cause reason={reason} has no \
                 completion timestamp — surfacing unconditionally (legacy row)"
            );
        }
    }

    // A reason this build has no code for is not a cause it can render.
    let code = FailureCode::from_str(reason)?;
    // Trust the persisted class when present and parseable; otherwise derive
    // from the code (keeps a forward-compatible default if the column is NULL
    // on an older row).
    let mut failure = PipelineFailure::new(code);
    if let Some(c) = class.as_deref() {
        if c == "transient" {
            failure.class = FailureClass::Transient;
        } else if c == "unrecoverable" {
            failure.class = FailureClass::Unrecoverable;
        }
    }
    Some(failure)
}
