use super::*;

/// Human-readable reason stamped on a run row that the [`RunRowFinalizer`]
/// drop-guard reconciles because its run future was dropped mid-flight (harness
/// tool abort, chat turn end, runtime shutdown, panic) before any terminal
/// write landed. Surfaced verbatim in the run-details sidebar (bug B42c) so a
/// cancelled/timed-out run reads as interrupted rather than a blank spinner.
pub(super) const INTERRUPTED_DROP_REASON: &str =
    "Run interrupted before completion — it was cancelled, timed out, or the app shut down mid-run.";

/// Cancellation-safe finalizer for a live `flow_runs` row (bug B42).
///
/// While a run's engine future is awaiting, dropping that future — the harness
/// 120s tool abort, a chat turn ending, tokio runtime shutdown, or a panic —
/// would otherwise leave the row wedged at `status="running"`, `error=NULL`,
/// `steps=[]` forever, which the run-details sidebar renders as a perpetual
/// blank spinner. Held across the await, this guard writes a terminal
/// `"interrupted"` status + human reason on `Drop` UNLESS it has been
/// explicitly [`disarm`](Self::disarm)ed after a real terminal write. The
/// `armed` flag is a single-task `Cell` (the guard never crosses tasks by
/// reference), so the type stays `Send` for `tokio::spawn`.
pub(super) struct RunRowFinalizer {
    config: Arc<Config>,
    thread_id: String,
    flow_id: String,
    armed: std::cell::Cell<bool>,
}

impl RunRowFinalizer {
    pub(super) fn new(config: Arc<Config>, thread_id: &str, flow_id: &str) -> Self {
        Self {
            config,
            thread_id: thread_id.to_string(),
            flow_id: flow_id.to_string(),
            armed: std::cell::Cell::new(true),
        }
    }

    /// Disarm the guard after a real terminal write (success/failure/cancel/
    /// pause) has already finalized the row, so `Drop` becomes a no-op.
    pub(super) fn disarm(&self) {
        self.armed.set(false);
    }
}

impl Drop for RunRowFinalizer {
    fn drop(&mut self) {
        if !self.armed.get() {
            return;
        }
        tracing::warn!(
            target: "flows",
            flow_id = %self.flow_id,
            thread_id = %self.thread_id,
            "[flows] RunRowFinalizer: run future dropped before settling — reconciling orphaned 'running' row to 'interrupted'"
        );
        // Preserve whatever steps the live observer already persisted.
        let observed = current_persisted_steps(&self.config, &self.thread_id);
        finish_flow_run_row(
            &self.config,
            &self.thread_id,
            &self.flow_id,
            "interrupted",
            &observed,
            &[],
            Some(INTERRUPTED_DROP_REASON),
            None,
        );
        // Keep the flow-definition summary in step with the row, exactly as the
        // success/failure/cancel arms and the boot sweep do — otherwise the
        // runs list keeps advertising the *previous* run's `last_status` /
        // `last_run_at` for a flow whose latest run was interrupted.
        // `record_run` is synchronous, so it is safe in `Drop`.
        if let Err(e) = store::record_run(&self.config, &self.flow_id, "interrupted") {
            tracing::warn!(
                target: "flows",
                flow_id = %self.flow_id,
                thread_id = %self.thread_id,
                error = %e,
                "[flows] RunRowFinalizer: failed to update flow summary for interrupted run"
            );
        }
    }
}

/// RFC3339 instant at which THIS process first entered the flow-run lifecycle —
/// the floor the boot orphan sweep (bug B42) uses to bound its candidate set.
///
/// Initialized on first touch by whichever comes first: [`start_flow_run_row`]
/// (which forces it *before* stamping the row it is about to insert) or
/// [`sweep_orphaned_running_runs_on_boot`]. Either ordering yields the same
/// invariant — **every `flow_runs` row this process inserts has
/// `started_at >= *PROCESS_RUN_FLOOR`** — so a sweep restricted to
/// `started_at < *PROCESS_RUN_FLOOR` provably only ever sees rows left behind by
/// a *prior* process.
///
/// The floor makes that guarantee structural rather than a consequence of
/// registration ordering. `run_registry::is_in_flight` alone once left a window
/// — the entry points used to insert the `running` row before `run_flow_body`
/// registered, so a live run was briefly `running`-but-not-in-flight, and
/// sweeping it there would `drop_checkpoint` it mid-run (unrecoverable, unlike
/// the status, which the live run's own terminal write would fix). Registration
/// has since moved ahead of the insert, closing that window at the source too;
/// the floor stays because it holds regardless of what future callers do with
/// that ordering.
pub(super) static PROCESS_RUN_FLOOR: LazyLock<String> = LazyLock::new(|| Utc::now().to_rfc3339());

/// Best-effort insert of the initial `"running"` `flow_runs` row. Logged,
/// never fails the run — run-history persistence is an observability aid,
/// not a correctness requirement of the run itself.
pub(super) fn start_flow_run_row(config: &Config, thread_id: &str, flow_id: &str) {
    // Anchor the boot-sweep floor BEFORE stamping this row, so this row's
    // `started_at` can never precede it. See [`PROCESS_RUN_FLOOR`].
    LazyLock::force(&PROCESS_RUN_FLOOR);
    let started_at = Utc::now().to_rfc3339();
    if let Err(e) = store::insert_flow_run(config, thread_id, flow_id, thread_id, &started_at) {
        tracing::warn!(target: "flows", flow_id, thread_id, error = %e, "[flows] failed to persist flow run start");
    }
}

/// Best-effort finalization of a `flow_runs` row. Logged, never fails the
/// run (see [`start_flow_run_row`]).
///
/// `graph_hash` (T-M1) should be `Some(hash)` only on the write that parks the
/// row (`status == "pending_approval"`) — every other caller passes `None`,
/// which clears any stale pin now that the row is leaving (or never entered)
/// `pending_approval`. See [`compute_graph_hash`] and `store::finish_flow_run`.
pub(super) fn finish_flow_run_row(
    config: &Config,
    thread_id: &str,
    flow_id: &str,
    status: &str,
    steps: &[FlowRunStep],
    pending_approvals: &[String],
    error: Option<&str>,
    graph_hash: Option<&str>,
) -> bool {
    let finished_at = Utc::now().to_rfc3339();
    match store::finish_flow_run(
        config,
        thread_id,
        status,
        &finished_at,
        steps,
        pending_approvals,
        error,
        graph_hash,
    ) {
        Err(e) => {
            tracing::warn!(target: "flows", thread_id, status, error = %e, "[flows] failed to persist flow run finish");
            return false;
        }
        // The guarded UPDATE (R-M2) matched nothing: the row had already
        // settled to a terminal status before this write. Whoever settled it
        // first also published `FlowRunFinished`, so publishing again here
        // would emit a second terminal event for one run. Report the no-op
        // instead of pretending the write landed.
        Ok(false) => {
            tracing::warn!(
                target: "flows",
                flow_id,
                thread_id,
                attempted_status = status,
                "[flows] finish_flow_run_row: row already terminal — refusing to overwrite a settled run"
            );
            return false;
        }
        Ok(true) => {}
    }

    // `status` can be `"pending_approval"` here (see `finalize_terminal_status`)
    // when the run merely paused at a gate — that isn't a finish. `flows_resume`
    // later settles under the SAME `thread_id`/`run_id`, and `useFlowRunFinished`
    // de-dupes delivered events by `${flow_id}:${run_id}` (needed because the
    // socket bridge re-emits this event under two aliases and must collapse
    // them into one `onFinish` call). Publishing here for a pause would poison
    // that dedup cache, so the real completion event after resume would be
    // dropped as an "alias replay" and the run could stay stale in the runs
    // list until the 30s poll backstop (Codex review, PR #5115). Gate the
    // publish to actual terminal statuses; the row itself is still written
    // above so poll-based fallbacks (list/get RPCs) see the paused state
    // either way.
    if status == "pending_approval" {
        tracing::debug!(
            target: "flows",
            flow_id,
            thread_id,
            status,
            "[flows] finish_flow_run_row: run paused for approval — not a finish, skipping FlowRunFinished"
        );
        return true;
    }

    tracing::debug!(
        target: "flows",
        flow_id,
        thread_id,
        status,
        "[flows] finish_flow_run_row: publishing FlowRunFinished"
    );
    crate::core::bus::BUS.publish(crate::core::events::DomainEvent::FlowRunFinished {
        flow_id: flow_id.to_string(),
        run_id: thread_id.to_string(),
        status: status.to_string(),
    });
    true
}

/// Reconstructs a lean per-node step list from a settled run's
/// `output["nodes"]` map.
///
/// As of issue G2 (live run observation) this is no longer the primary source
/// of run steps — `flows::observability::FlowRunObserver` persists each step
/// live as it finishes (with real `status`/`duration_ms`). This reconstruction
/// is now only a **fallback**, used by [`settle_steps`] to fill in any node the
/// observer didn't emit an `on_step_finish` for (notably the trigger node),
/// and as the whole-run source when the observer saw nothing at all.
fn reconstruct_steps(output: &Value) -> Vec<FlowRunStep> {
    let Some(nodes) = output.get("nodes").and_then(Value::as_object) else {
        return Vec::new();
    };
    nodes
        .iter()
        .map(|(node_id, slot)| FlowRunStep {
            node_id: node_id.clone(),
            output: slot.get("items").cloned().unwrap_or(Value::Null),
            port: slot.get("port").and_then(Value::as_str).map(str::to_string),
            // Reconstructed post-hoc: no live status/timing (see FlowRunStep).
            status: None,
            duration_ms: None,
            diagnostics: Vec::new(),
        })
        .collect()
}

/// Reads back whatever steps the live [`FlowRunObserver`] has already persisted
/// onto the run's row. Best-effort: a read failure yields an empty list (the
/// caller still writes a terminal row), never propagating an error into the
/// run's settle path.
///
/// [`FlowRunObserver`]: crate::flows::tinyflows::observability::FlowRunObserver
pub(super) fn current_persisted_steps(config: &Config, run_id: &str) -> Vec<FlowRunStep> {
    store::get_flow_run(config, run_id)
        .ok()
        .flatten()
        .map(|run| run.steps)
        .unwrap_or_default()
}

/// Assembles the final step list to persist at settle: the live steps the
/// observer already recorded (carrying real `status`/`duration_ms`), plus any
/// node present in the post-hoc [`reconstruct_steps`] projection that the
/// observer never emitted a step for — the trigger node, or (defensively) an
/// observer that missed a step. If the observer recorded nothing at all
/// (e.g. a run that paused immediately at a gate before any node finished),
/// falls back wholesale to the reconstruction.
pub(super) fn settle_steps(config: &Config, run_id: &str, output: &Value) -> Vec<FlowRunStep> {
    let reconstructed = reconstruct_steps(output);
    let persisted = current_persisted_steps(config, run_id);
    if persisted.is_empty() {
        tracing::debug!(
            target: "flows",
            run_id,
            reconstructed = reconstructed.len(),
            "[flows] settle_steps: no live-observed steps — using post-hoc reconstruction"
        );
        return reconstructed;
    }
    let mut merged = persisted;
    let mut filled = 0usize;
    for step in reconstructed {
        if !merged.iter().any(|s| s.node_id == step.node_id) {
            merged.push(step);
            filled += 1;
        }
    }
    tracing::debug!(
        target: "flows",
        run_id,
        step_count = merged.len(),
        filled_from_reconstruction = filled,
        "[flows] settle_steps: merged live-observed steps with post-hoc reconstruction"
    );
    merged
}

/// Degrades a would-be `"completed"` status: `"failed"` if any settled step
/// errored, `"completed_with_warnings"` if any carries null-resolution
/// diagnostics, else `"completed"`.
///
/// Called only once the run has no `pending_approvals` left — precedence
/// against that case is handled by the caller (`pending_approval` always
/// wins over any of these).
pub(super) fn degrade_completed_status(steps: &[FlowRunStep]) -> &'static str {
    if steps.iter().any(|s| s.status.as_deref() == Some("error")) {
        return "failed";
    }
    if steps.iter().any(|s| !s.diagnostics.is_empty()) {
        "completed_with_warnings"
    } else {
        "completed"
    }
}

/// Names the node(s) whose step settled with `status == "error"` — the
/// engine's `ExecutionStep` carries no error message of its own for a step
/// that failed under an `on_error: "continue"`/`"route"` policy (it only
/// fails the *run* future, and so gets an actual error string, when the
/// policy is `"stop"`), so this is the best available detail for
/// [`FlowRun::error`] when [`degrade_completed_status`] degrades to
/// `"failed"` without an outer run-future `Err`.
pub(super) fn failed_step_error_summary(steps: &[FlowRunStep]) -> Option<String> {
    let failed_nodes: Vec<&str> = steps
        .iter()
        .filter(|s| s.status.as_deref() == Some("error"))
        .map(|s| s.node_id.as_str())
        .collect();
    if failed_nodes.is_empty() {
        None
    } else {
        Some(format!(
            "node(s) failed after retries: {}",
            failed_nodes.join(", ")
        ))
    }
}

/// Computes a settled run's terminal status and, when that status is
/// `"failed"`, an accompanying error message — shared by `flows_run` and
/// `flows_resume` so the two call sites can't drift on the
/// `pending_approval` > `degrade_completed_status` precedence or forget to
/// populate [`FlowRun::error`] (its doc contract: "Error message when
/// `status == \"failed\"`") for a run that degraded via a settled step error
/// rather than an outer run-future `Err`.
pub(super) fn finalize_terminal_status(
    settled: &[FlowRunStep],
    pending_approvals: &[String],
) -> (&'static str, Option<String>) {
    if !pending_approvals.is_empty() {
        return ("pending_approval", None);
    }
    let status = degrade_completed_status(settled);
    let error = if status == "failed" {
        failed_step_error_summary(settled)
    } else {
        None
    };
    (status, error)
}
