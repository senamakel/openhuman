//! `memory_tree_pipeline_status` and `memory_tree_doctor` (#1856 Part 1,
//! #002 FR-009): the aggregate health snapshot the UI status panel renders,
//! and the one-shot diagnostic behind it.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::rpc::RpcOutcome;

use super::backfill::{queue_stats, store_stats};
use super::retry_failed::latest_failed_job_failure;
use super::stall::{
    compute_dir_size_bytes, derive_pipeline_status, queue_idle_ms, queue_is_stalled,
};

/// Per-status counters for the `mem_tree_jobs` table — snapshot returned by
/// the `memory_tree_pipeline_status` RPC. Only the three states the status
/// panel surfaces are exposed; `done` / `cancelled` are intentionally
/// omitted to keep the wire payload small.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineJobCounts {
    /// Jobs queued and waiting for a worker (`status = 'ready'`).
    pub ready: u64,
    /// Jobs currently being processed by a worker (`status = 'running'`).
    pub running: u64,
    /// Jobs that exhausted retries and remain in the table for diagnosis
    /// (`status = 'failed'`).
    pub failed: u64,
}

/// Response from the `memory_tree_pipeline_status` RPC (#1856 Part 1).
///
/// Aggregates "is the Memory Tree healthy?" signals into a single payload
/// the UI status panel can render without secondary fetches:
///
/// - `status` is a coarse, UI-shaped string (`running`/`paused`/`syncing`/
///   `error`/`idle`) derived from the other fields so the frontend stays
///   purely presentational.
/// - `wiki_size_bytes` is a recursive walk of the on-disk `wiki/` sub-tree
///   under the memory-tree content root; recomputed every call (cheap for
///   typical workspaces). The walk is scoped to `wiki/` so the figure
///   reflects the user-visible wiki only — not the sibling `raw/`,
///   `email/`, `chat/`, `document/` staging directories.
/// - `pipeline_jobs` is a snapshot of the queue — running > 0 implies
///   active sync, failed > 0 implies degraded.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineStatusResponse {
    /// Aggregated status string: `running` | `paused` | `syncing` |
    /// `degraded` | `error` | `idle`. Derivation:
    /// 1. `is_paused` (scheduler-gate `off`) wins → `paused`.
    /// 2. otherwise failed > 0 → `error`.
    /// 3. otherwise degraded (#002, recall/structure reduced) → `degraded`.
    /// 4. otherwise running > 0 → `syncing`.
    /// 5. otherwise total_chunks > 0 → `running`.
    /// 6. otherwise → `idle`.
    pub status: String,
    /// Optional human-readable reason — populated when status is
    /// `paused` or `error`. `None` otherwise.
    pub reason: Option<String>,
    /// Epoch milliseconds of the most-recent chunk timestamp across all
    /// sources. Zero when the store is empty.
    pub last_sync_ms: i64,
    /// Total `mem_tree_chunks` rows across all sources.
    pub total_chunks: u64,
    /// Recursive byte size of the on-disk `wiki/` sub-tree under the
    /// memory-tree content root. Zero when the `wiki/` directory does not
    /// exist yet or cannot be read. Scoped to `wiki/` so the value matches
    /// the user-visible "Wiki size" tile (#1856 follow-up).
    pub wiki_size_bytes: u64,
    /// Snapshot counts from `mem_tree_jobs`.
    pub pipeline_jobs: PipelineJobCounts,
    /// Convenience flag: at least one job is currently `running`.
    pub is_syncing: bool,
    /// Convenience flag: scheduler-gate is in `off` mode, so all LLM-bound
    /// background work is paused cooperatively.
    pub is_paused: bool,
    /// The scheduler gate's *live* verdict: `true` while its policy is
    /// `Paused`, whichever mode is configured — in `auto` that is on battery
    /// with `require_ac_power`, under CPU pressure, or while signed out — so
    /// every LLM-bound worker, the embed backfill included, sits behind
    /// `wait_for_capacity` right now. `is_paused` stays the configured `off`
    /// mode (the panel's toggle). Additive: `#[serde(default)]` keeps older
    /// clients deserialising the response (openhuman#6025 review).
    #[serde(default)]
    pub gate_paused: bool,
    /// Why the gate is paused, when it is: `user_disabled`, `on_battery`,
    /// `cpu_pressure`, `signed_out` or `unknown` — the module host's
    /// `scheduler_policy` spells them the same way. `None` while running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_pause_reason: Option<String>,
    /// The #5324 stall verdict as a flag: eligible work has waited at least
    /// six hours without any job settling. `status` already reads `degraded`
    /// for it, but a stalled `reembed_backfill` row still keeps the backfill
    /// snapshot `in_progress`, and a row must not promise vectors "shortly"
    /// while the queue is provably not draining (openhuman#6025 review).
    /// Additive: `#[serde(default)]` keeps older clients deserialising.
    #[serde(default)]
    pub queue_stalled: bool,
    /// #002 (FR-002/FR-004): "the pipeline ran but output quality is reduced"
    /// — `semantic_recall` true when embeddings were skipped (no usable
    /// provider, so recall falls back to recency), `structure` true when
    /// extraction yielded nothing across the board (empty wiki). Carries the
    /// typed `cause` so the UI can render an actionable remediation. Additive:
    /// `#[serde(default)]` keeps older clients deserialising the response.
    #[serde(default)]
    pub degraded: crate::memory::tree::health::DegradedState,
    /// #002 (FR-004): the single first blocking/most-significant cause, as a
    /// typed failure with an i18n remediation key. Populated from a failed
    /// job's classified reason or the active degradation cause; `None` when
    /// the pipeline is healthy. The frontend renders this verbatim (resolving
    /// `remediation_key`) instead of re-deriving a cause from raw counters.
    #[serde(default)]
    pub first_blocking_cause: Option<crate::memory::tree::health::PipelineFailure>,
    /// #002 (FR-010 / US5): fraction of chunks with ≥1 indexed entity, in
    /// `[0.0, 1.0]`. Near 0 with `total_chunks > 0` means extraction is
    /// producing no structure (the "empty-but-built wiki"). `None` when the
    /// metric could not be measured (DB read error) — deliberately distinct
    /// from a genuine `Some(0.0)` so the status surface never misreports a
    /// broken measurement path as a structure failure. Additive
    /// (`#[serde(default)]` → `None` for older clients).
    #[serde(default)]
    pub extraction_coverage: Option<f32>,
    /// openhuman#5820: the most recent corrupt-store quarantine in this
    /// workspace, derived from disk (`memory_tree/chunks.db.corrupt-<ts>`),
    /// so it survives restarts and reaches a renderer that was not connected
    /// when the quarantine happened. `None` when nothing was ever quarantined.
    /// Reported until the rebuilt store holds a chunk again (`resynced`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<QuarantineStatus>,
}

/// A corrupt-store quarantine as the status surface reports it (openhuman#5820).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineStatus {
    /// Epoch milliseconds of the quarantine, parsed from the file name's UTC
    /// timestamp (`chunks.db.corrupt-%Y%m%dT%H%M%SZ`).
    pub quarantined_at_ms: i64,
    /// The preserved copy of the damaged database. Local to this machine and
    /// shown only to its own user, so the user can hand it to recovery tooling.
    pub quarantined_path: String,
    /// Whether the rebuilt store holds any chunk again. The quarantine leaves
    /// an empty schema, so a non-empty store means the user has re-synced
    /// and the notice can retire. Deliberately not a timestamp comparison:
    /// chunk timestamps are *content* time (a mail's `sent_at`, a file's
    /// `modified_at`), so restored history predates the quarantine forever.
    pub resynced: bool,
}

/// Newest `chunks.db.corrupt-<ts>` under `<workspace>/memory_tree`, if any.
///
/// Disk is the durable record: the engine's quarantine renames the damaged
/// file beside the store and never deletes it, so a status read after a
/// restart — or from a renderer that missed the live event — still finds it.
/// Side-file quarantines (`chunks.db-wal.corrupt-…`) do not match the prefix.
///
/// `pub(super)` — reused verbatim by tests declared directly under `rpc`.
pub(super) fn latest_quarantine(
    workspace_dir: &std::path::Path,
    total_chunks: u64,
) -> Option<QuarantineStatus> {
    const PREFIX: &str = "chunks.db.corrupt-";
    let dir = workspace_dir.join("memory_tree");
    let newest = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let stamp = name.strip_prefix(PREFIX)?.to_owned();
            let at = chrono::NaiveDateTime::parse_from_str(&stamp, "%Y%m%dT%H%M%SZ").ok()?;
            Some((at.and_utc().timestamp_millis(), entry.path()))
        })
        .max_by_key(|(at, _)| *at)?;
    let (quarantined_at_ms, path) = newest;
    Some(QuarantineStatus {
        quarantined_at_ms,
        quarantined_path: path.display().to_string(),
        resynced: total_chunks > 0,
    })
}

/// The scheduler gate's live verdict, mapped for the wire.
///
/// `is_paused` is the configured `off` mode (the panel's toggle); this is
/// what the workers see right now. In `auto` the gate also pauses on battery
/// with `require_ac_power`, under CPU pressure, or while signed out, and an
/// armed embed backfill then waits in `wait_for_capacity` with its
/// `in_progress` flag still up — a row must not promise vectors "shortly" on
/// that flag alone (openhuman#6025 review). Reason slugs match the module
/// host's `scheduler_policy` so both surfaces spell a pause the same way.
///
/// `pub(super)` — reused verbatim by tests declared directly under `rpc`.
pub(super) fn gate_pause_state(
    policy: crate::cron::scheduler_gate::Policy,
) -> (bool, Option<String>) {
    use crate::cron::scheduler_gate::{PauseReason, Policy};
    match policy {
        Policy::Paused { reason } => (
            true,
            Some(
                match reason {
                    PauseReason::UserDisabled => "user_disabled",
                    PauseReason::OnBattery => "on_battery",
                    PauseReason::CpuPressure => "cpu_pressure",
                    PauseReason::SignedOut => "signed_out",
                    PauseReason::Unknown => "unknown",
                }
                .to_string(),
            ),
        ),
        Policy::Aggressive | Policy::Normal | Policy::Throttled => (false, None),
    }
}

/// Aggregates `list_sources` + `count_by_status` + a recursive disk-size
/// probe into the [`PipelineStatusResponse`] the UI status panel renders.
/// All blocking work is dispatched onto `spawn_blocking` so the async
/// runtime isn't held during SQLite or filesystem I/O.
pub async fn pipeline_status_rpc(
    config: &Config,
) -> Result<RpcOutcome<PipelineStatusResponse>, String> {
    use tinymemory_api::host::SchedulerGateMode;

    log::debug!("[memory-tree][rpc] pipeline_status: entry");

    // Chunk aggregates — count, extracted count and newest timestamp, in one
    // observation of the driver. Splitting them is what let a write land
    // between the count and the extracted count and report an extraction
    // coverage above 100%.
    let store = store_stats(config).await.map_err(|e| {
        log::warn!("[memory-tree][rpc] pipeline_status: {e}");
        e
    })?;
    let total_chunks = store.chunks;
    // The wire field is a plain `i64` where the driver answers `Option`, and
    // `0` is its established "never synced" value — an empty store has no
    // newest chunk, which is not a chunk stamped at the epoch.
    let last_sync_ms = store.most_recent_chunk_ms.unwrap_or(0).max(0);

    // Job counters — one observation of the queue, where this used to be five
    // separate reads at five instants. That mattered: `failed_unrecoverable`
    // is the #3365 left-right split (of the failed jobs, how many are the
    // hard, user-actionable kind vs transient ones that self-heal via
    // auto-requeue, since only the former escalates to `error`), and a retry
    // landing between the two reads could report more unrecoverable failures
    // than failures.
    //
    // #5324's stall signal comes from the same snapshot for the same reason —
    // an idle time computed against counts taken at a different instant reads
    // as a stall that never happened.
    let now_ms = chrono::Utc::now().timestamp_millis();
    let queue = queue_stats(config, None).await.map_err(|e| {
        log::warn!("[memory-tree][rpc] pipeline_status: {e}");
        e
    })?;
    let pipeline_jobs = PipelineJobCounts {
        ready: queue.ready,
        running: queue.running,
        failed: queue.failed,
    };
    let failed_unrecoverable = queue.failed_unrecoverable;
    let queue_idle_ms = queue_idle_ms(&queue, now_ms);
    let queue_stalled = queue_is_stalled(queue_idle_ms);

    // Disk size — best-effort. Permission errors etc. degrade to 0 with a
    // warn log rather than failing the whole RPC. Scoped to the `wiki/`
    // sub-directory so the tile lives up to its "Wiki size" label — the
    // sibling `raw/` / `email/` / `chat/` / `document/` staging directories
    // hold pre-canonicalised content and should not roll into the figure
    // surfaced to the user (#1856 CodeRabbit feedback).
    let wiki_root = config.memory_tree_content_root().join("wiki");
    let wiki_size_bytes = tokio::task::spawn_blocking(move || compute_dir_size_bytes(&wiki_root))
        .await
        .map_err(|e| {
            let msg = format!("pipeline_status size-walk join error: {e}");
            log::warn!("[memory-tree][rpc] pipeline_status: {msg}");
            msg
        })?;

    let is_paused = config.scheduler_gate.mode == SchedulerGateMode::Off;
    let (gate_paused, gate_pause_reason) =
        gate_pause_state(crate::cron::scheduler_gate::gate::current_policy());
    let is_syncing = pipeline_jobs.running > 0;

    // #002: read the degradation snapshot (set by the embed / extract stages)
    // so a half-working sync surfaces as `degraded` with a cause rather than a
    // misleading `running`. The structure-degraded latch is a liveness signal
    // ("the extraction model is timing out") kept honest at its source in the
    // driver's `extract::llm` — it self-clears on the next *completed*
    // extraction (#3365), so the status surface never consults the unrelated
    // `extraction_coverage` metric to second-guess it here.
    //
    // Asked of the driver rather than of this process's statics (#5560). Those
    // flags live inside whichever process ran the embed and extract stages, and
    // that is the module — a `cdylib` with its own statics — so the host-side
    // read answered all-clear no matter what the pipeline had done. Same class
    // of bug, and same fix, as `backfill_in_progress` above.
    //
    // Degraded to all-clear on a read failure, matching the two other reads
    // this handler must not fail on (`backfill_in_progress` and
    // `latest_failed_job_failure`): a status surface that errors tells the user
    // less than one reporting an undegraded store, and all-clear is what the
    // host-side read answered anyway.
    let degraded = crate::memory::tree::health::report::current_degraded_state(config)
        .await
        .unwrap_or_else(|error| {
            log::warn!("[memory-tree][rpc] pipeline_status: degraded state read failed: {error}");
            Default::default()
        });

    let (status, reason) = derive_pipeline_status(
        is_paused,
        config.scheduler_gate.mode,
        is_syncing,
        pipeline_jobs.failed,
        failed_unrecoverable,
        total_chunks,
        &degraded,
        queue_idle_ms,
    );

    // #002 first_blocking_cause (FR-004): the most-recent failed job's typed
    // reason, surfaced verbatim by the UI. Best-effort — log-then-drop, so a
    // read failure is distinguishable in the log from "no blocking cause"
    // while never failing the polled status RPC. The `spawn_blocking` this
    // used to sit in is the driver's business now.
    let latest_failure = latest_failed_job_failure(config).await.unwrap_or_else(|e| {
        log::warn!(
            "[memory-tree][rpc] pipeline_status: latest_failed_job_failure read failed: {e}"
        );
        None
    });

    // #002 extraction_coverage (FR-010/US5): fraction of chunks with
    // structure, surfaced as its own display metric — deliberately NOT folded
    // into the status pill (#3365: coverage is a cumulative measure, unrelated
    // to the live structure-degraded liveness signal).
    //
    // Derived from the `store_stats` snapshot above, so the numerator and
    // denominator are one observation and the fraction cannot exceed 1.0 —
    // which two separate reads could produce, and did.
    //
    // An empty store still reports `Some(0.0)`, matching what this returned
    // before. `None` here has always meant "unavailable", and while `0.0` for
    // a store with nothing to extract is arguably the wrong reading, changing
    // it is a decision about what the panel shows, not a consequence of moving
    // the read behind the contract.
    let extraction_coverage = Some(if store.chunks == 0 {
        0.0
    } else {
        store.chunks_with_structure as f32 / store.chunks as f32
    });

    // A hard failed-job reason is more urgent than a soft degradation; fall
    // back to the active degradation cause, then `None` when healthy.
    let first_blocking_cause = latest_failure.or_else(|| degraded.cause.clone());

    // openhuman#5820: disk-derived so it is durable and replayable; "resynced"
    // reads the same `store` observation as the chunk tile, so the two agree.
    let quarantine = latest_quarantine(config.workspace_dir.as_path(), total_chunks);
    if let Some(q) = &quarantine {
        log::debug!(
            "[memory-tree][rpc] pipeline_status: quarantine at={} resynced={} path={}",
            q.quarantined_at_ms,
            q.resynced,
            q.quarantined_path
        );
    }

    let payload = PipelineStatusResponse {
        status: status.clone(),
        reason: reason.clone(),
        last_sync_ms,
        total_chunks,
        wiki_size_bytes,
        pipeline_jobs,
        is_syncing,
        is_paused,
        gate_paused,
        gate_pause_reason,
        queue_stalled,
        degraded,
        first_blocking_cause,
        extraction_coverage,
        quarantine,
    };

    log::debug!(
        "[memory-tree][rpc] pipeline_status: ok status={status} total_chunks={total_chunks} wiki_size_bytes={wiki_size_bytes} ready={r} running={n} failed={f} reason={reason:?}",
        r = payload.pipeline_jobs.ready,
        n = payload.pipeline_jobs.running,
        f = payload.pipeline_jobs.failed,
    );

    Ok(RpcOutcome::single_log(
        payload,
        format!(
            "memory_tree: pipeline_status status={status} total_chunks={total_chunks} is_paused={is_paused} is_syncing={is_syncing}",
        ),
    ))
}

/// `memory_tree_doctor` RPC handler (#002 FR-009). Runs the one-shot
/// pipeline diagnostic and returns the
/// [`DoctorReport`](crate::memory::tree::health::report::DoctorReport)
/// — per-stage health, the first blocking cause, the degraded snapshot, and
/// counters. Exposed for the agent tool + CLI so the agent can self-diagnose an
/// empty/stalled wiki.
///
/// The pass runs inside the driver now (`MemoryMaintenance::diagnose`), which
/// is also where the blocking SQLite reads it makes have always been — this
/// host no longer dispatches a blocking task for them, and the report's shape
/// is unchanged.
pub async fn doctor_rpc(
    config: &Config,
) -> Result<RpcOutcome<crate::memory::tree::health::report::DoctorReport>, String> {
    let report = crate::memory::tree::health::report::run_doctor(config).await;
    let summary = if report.healthy {
        "memory_tree: doctor — healthy".to_string()
    } else {
        format!(
            "memory_tree: doctor — first_blocking_cause={}",
            report
                .first_blocking_cause
                .as_ref()
                .map(|f| f.code.as_str())
                .unwrap_or("unknown")
        )
    };
    Ok(RpcOutcome::single_log(report, summary))
}
