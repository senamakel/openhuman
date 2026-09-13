//! Pure derivations used by [`super::pipeline_status`]: the disk-size walk,
//! the #5324 queue-stall verdict, and the `(status, reason)` precedence rule.
//! Kept side-effect-free (besides the disk walk and its own logging) so the
//! unit tests can exercise the precedence rules without spinning up a store.

/// Recursive byte-count of files under `root`. Returns `0` when the root
/// does not exist or any traversal error occurs (best-effort; the status
/// panel is a UI convenience, not an audit surface).
pub(super) fn compute_dir_size_bytes(root: &std::path::Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(e) if e.file_type().is_file() => {
                if let Ok(meta) = e.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
            Ok(_) => {}
            Err(err) => {
                // Both `err.path()` and `walkdir::Error`'s `Display` impl
                // embed the absolute on-disk path (which lives under the
                // user's home directory), so we redact: log only whether a
                // path was attached and the underlying `io::ErrorKind`.
                // That's enough for diagnosis while keeping the user's
                // workspace layout out of the log file.
                log::warn!(
                    "[memory-tree][rpc] pipeline_status: dir walk error has_path={} kind={:?}",
                    err.path().is_some(),
                    err.io_error().map(|e| e.kind())
                );
            }
        }
    }
    total
}

/// #5324: how long the queue may hold eligible work without settling a single
/// job before the pipeline is reported as `degraded` rather than
/// `running`/`idle`.
///
/// A working pipeline settles jobs continuously, so this is idle time, not
/// backlog depth — a deep-but-draining backfill never approaches it. Six hours
/// is far outside a normal flush window (minutes) yet well inside the "broken
/// for a month" window the issue describes, so it cannot fire on a busy
/// machine or a laptop that was asleep for an hour.
pub(crate) const QUEUE_STALL_THRESHOLD_MS: i64 = 6 * 60 * 60 * 1000;

/// The #5324 stall verdict on its own: eligible work has been waiting for at
/// least [`QUEUE_STALL_THRESHOLD_MS`] without any job settling. Shared by the
/// status precedence below and by the `queue_stalled` flag the response
/// carries, so the two cannot disagree (openhuman#6025 review).
pub(crate) fn queue_is_stalled(queue_idle_ms: Option<i64>) -> bool {
    queue_idle_ms.is_some_and(|idle| idle >= QUEUE_STALL_THRESHOLD_MS)
}

/// Pure derivation of `(status, reason)` from raw signals. Split out so the
/// unit tests can exercise the precedence rules without spinning up a
/// store.
///
/// `queue_idle_ms` is how long the queue has held eligible work without
/// settling any job, or `None` when no eligible work is waiting (or the
/// metric could not be read).
///
/// `pub(super)` — reused verbatim by tests declared directly under `rpc`.
pub(super) fn derive_pipeline_status(
    is_paused: bool,
    mode: tinymemory_api::host::SchedulerGateMode,
    is_syncing: bool,
    failed: u64,
    failed_unrecoverable: u64,
    total_chunks: u64,
    degraded: &crate::memory::tree::health::DegradedState,
    queue_idle_ms: Option<i64>,
) -> (String, Option<String>) {
    if is_paused {
        return (
            "paused".to_string(),
            Some(format!("scheduler gate mode = {}", mode.as_str())),
        );
    }
    // Host storage is unusable (EIO/ENOSPC/EROFS on the memory_tree path). This
    // is a foundational, unrecoverable error — the DB can't even open, so it
    // outranks the per-content recall/structure degradation below AND fires
    // regardless of `total_chunks` (on a dead disk we may not be able to count
    // chunks at all). Only the user can fix it (reseat/replace/free storage);
    // the actionable remediation text rides the `StorageUnavailable`
    // remediation key surfaced by the doctor's `first_blocking_cause`.
    if degraded.storage {
        return (
            "error".to_string(),
            Some("memory storage unavailable — check your disk / SD card".to_string()),
        );
    }
    // #3365: split the failed bucket by class. Only an UNRECOVERABLE failure
    // (budget / auth / dim-mismatch) is a hard `error` the user must act on —
    // it stays parked and can't self-heal. Transient failures are auto-requeued
    // by `requeue_transient_failed`, so they must NOT escalate to `error`; they
    // fall through to `degraded` ("failed, retrying") below. This fixes the prior
    // `failed > 0 → error` that flashed a scary error for a job about to retry.
    if failed_unrecoverable > 0 {
        return (
            "error".to_string(),
            Some(format!(
                "{failed_unrecoverable} unrecoverable failure(s) need action"
            )),
        );
    }
    // #5324: the queue is accepting work but not draining it. This is the
    // "silently broken for a month" shape — new files keep getting detected
    // and queued, health checks keep reporting `ok` because the process is
    // alive, and nothing ever becomes searchable memory. Liveness is not
    // output, so a queue whose oldest ready job has been waiting past the
    // threshold reports `degraded`, never `running`/`idle`.
    //
    // Sits below `error` (a typed unrecoverable failure is the more specific
    // diagnosis and carries its own remediation) and above the recall/structure
    // degradation, and is deliberately NOT gated on `total_chunks` — a queue
    // that never drained has no chunks to gate on, which is exactly the case
    // that must not read as `idle`.
    if queue_is_stalled(queue_idle_ms) {
        let hours = queue_idle_ms.unwrap_or(0) / (60 * 60 * 1000);
        return (
            "degraded".to_string(),
            Some(format!(
                "queue has not completed any job in {hours}h — memory is not growing"
            )),
        );
    }
    // #002 (FR-005): "degraded" sits below error but above syncing/running —
    // the pipeline is making progress, but recall/structure is reduced (or some
    // jobs failed transiently and are retrying) and the user should be told why.
    // Beats syncing/running so a half-working sync isn't reported as plain
    // "running"/"syncing".
    //
    // Only fires when there are chunks: degraded recall/structure is only
    // meaningful when there's actual content affected. An empty workspace with
    // a misconfigured embedder should show "idle" (nothing to recall) rather
    // than "degraded" (recall is broken for existing content).
    //
    // `failed` here is transient-only — any unrecoverable failure returned
    // `error` above, so a non-zero `failed` at this point means jobs that will
    // be auto-requeued.
    if (degraded.is_degraded() || failed > 0) && total_chunks > 0 {
        let mut parts: Vec<String> = Vec::new();
        if degraded.semantic_recall {
            parts.push("semantic recall disabled".to_string());
        }
        if degraded.structure {
            parts.push("wiki structure incomplete".to_string());
        }
        if failed > 0 {
            parts.push(format!("{failed} job(s) failed, retrying"));
        }
        return ("degraded".to_string(), Some(parts.join("; ")));
    }
    if is_syncing {
        return ("syncing".to_string(), None);
    }
    if total_chunks > 0 {
        return ("running".to_string(), None);
    }
    ("idle".to_string(), None)
}

/// #5324: how long the queue has been sitting on eligible work without
/// finishing anything, or `None` when there is no eligible work waiting.
///
/// This is the "queued but never processed" signal. Getting the predicate
/// right matters more than it looks, because the naive versions produce false
/// alarms for exactly the heavy users this issue is about:
///
/// - **Not** `MIN(created_at_ms)` over all `ready` rows. `mark_deferred` parks
///   a backing-off job by leaving `status = 'ready'` and pushing
///   `available_at_ms` forward, so deferred work would count as waiting when
///   it is deliberately asleep.
/// - **Not** the age of the oldest eligible row either. A re-embed backfill
///   enqueues thousands of rows in one burst; six hours into a perfectly
///   healthy drain of a 68k-chunk workspace, the oldest un-drained row is by
///   definition hours old. That would flag the exact case the issue's reporter
///   was in — a big, slow, *working* backfill — as broken.
///
/// So the measure is **idle time, not backlog age**: how long since the queue
/// last settled *any* job. A pipeline making progress refreshes
/// `completed_at_ms` continuously no matter how deep the backlog is, while a
/// pipeline whose jobs all fail unrecoverably or whose worker never runs goes
/// quiet. `completed_at_ms` is stamped on failure as well as success, so a
/// fast-failing pipeline reports `error` (via `failed_unrecoverable`) rather
/// than being mislabelled as stalled.
///
/// Returns `Some(idle_ms)` only when eligible work is actually waiting — an
/// idle queue with nothing to do is not stalled, it is done. When nothing has
/// ever settled (fresh workspace whose worker has never run), idle time falls
/// back to how long the oldest eligible job has been waiting.
///
/// Derived from a snapshot rather than read on its own, so the eligible count
/// and the timestamps it is measured against come from one instant. Reading
/// them separately is what makes an idle window appear across a settle that
/// happened between two queries.
///
/// `pub(super)` — reused verbatim by tests declared directly under `rpc`.
pub(super) fn queue_idle_ms(
    queue: &crate::memory::api::provider::types::QueueStats,
    now_ms: i64,
) -> Option<i64> {
    // Nothing eligible is waiting ⇒ nothing is being held up.
    if queue.eligible_now == 0 {
        return None;
    }
    let (last_settled_ms, oldest_eligible_ms) = (queue.last_completed_ms, queue.oldest_eligible_ms);
    // Idle time is "how long since the queue last made progress on the work
    // that is waiting *now*" — so start the clock at the LATER of the last
    // settle and the oldest eligible job's arrival. Using `last_settled_ms`
    // alone (`.or`) mis-reads a real shape: if the queue drained everything,
    // sat empty for days, then a fresh job arrives, the stale completion is
    // hours/days old while the new work is seconds old. Taking the max means
    // freshly-enqueued work starts its own idle window instead of inheriting
    // an ancient completion, so a just-arrived job can't be flagged `degraded`
    // before the worker has had a chance to touch it. Fall back to the oldest
    // eligible job's wait when the queue has never settled a job at all.
    let reference_ms = match (last_settled_ms, oldest_eligible_ms) {
        (Some(last_settled), Some(oldest_eligible)) => Some(last_settled.max(oldest_eligible)),
        (Some(last_settled), None) => Some(last_settled),
        (None, Some(oldest_eligible)) => Some(oldest_eligible),
        (None, None) => None,
    };
    // Clamp at zero: clock skew / a future-dated row must read as "just now",
    // never as a negative age.
    reference_ms.map(|since| (now_ms - since).max(0))
}
