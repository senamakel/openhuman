//! The background job queue: the slow memory work TinyMemory hands back.
//!
//! `post_turn` and brain ingests return `BackgroundJob` values (belief builds,
//! deferred ingests) instead of running them. They are queued here, persisted
//! in `<workspace>/memory/jobs.json` so a restart loses none, de-duplicated
//! (two builds of one scope are one build), and run by the
//! `memory_background` cron job ([`run_due`]) once they are at least
//! `[memory.recall] build_delay_secs` old — a belief build reads the facts the
//! engine extracts from the writes, and that extraction lags behind them.
//!
//! An engine that cannot consolidate answers `Skipped`, so the same queue runs
//! on every engine; the hosted engine builds on its own schedule
//! (`Scheduled`).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tinymemory_api::Namespace;
use tinymemory_tools::{BackgroundJob, BackgroundRunner, JobOutcome, MemoryLayout};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::memory::engine;
use crate::memory::error::{MemoryError, MemoryResult};

/// The cron job that runs the queue.
pub const BACKGROUND_JOB: &str = "memory_background";

/// Minutes between queue runs.
pub const BACKGROUND_INTERVAL_MINS: u64 = 5;

/// Most finished runs kept for the UI.
const MAX_HISTORY: usize = 50;

/// Attempts before a failing job is dropped.
const MAX_ATTEMPTS: u32 = 5;

/// Serialises queue read-modify-writes in this process. Held across a run,
/// so two runs never take the same job.
static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// One queued job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedJob {
    /// Stable id: a digest of the job and its root, so a duplicate merges.
    pub id: String,
    /// The layout root it runs under.
    pub root: String,
    /// The work.
    pub job: BackgroundJob,
    /// When it was first queued.
    pub queued_at: DateTime<Utc>,
    /// Failed runs so far.
    #[serde(default)]
    pub attempts: u32,
    /// The last failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// One finished run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRun {
    /// The job's id.
    pub id: String,
    /// `build_beliefs` or `ingest_brain`.
    pub job: String,
    /// The layout root.
    pub root: String,
    /// When it ran.
    pub ran_at: DateTime<Utc>,
    /// `done`, `started`, `scheduled`, `skipped` or `failed`.
    pub outcome: String,
    /// Why it was skipped or failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Beliefs built, when the engine says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built: Option<usize>,
    /// Items stored by an ingest.
    #[serde(default)]
    pub stored: usize,
}

/// The persisted queue.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JobQueue {
    /// Jobs waiting to run.
    #[serde(default)]
    pub pending: Vec<QueuedJob>,
    /// The latest finished runs, newest first.
    #[serde(default)]
    pub history: Vec<JobRun>,
}

fn path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("memory").join("jobs.json")
}

fn read(workspace_dir: &Path) -> JobQueue {
    std::fs::read_to_string(path(workspace_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write(workspace_dir: &Path, queue: &JobQueue) {
    let file = path(workspace_dir);
    let result = serde_json::to_vec_pretty(queue)
        .map_err(std::io::Error::other)
        .and_then(|json| crate::memory::files::write_private(&file, &json));
    if let Err(error) = result {
        tracing::warn!(%error, "[memory:jobs] writing the job queue failed");
    }
}

fn job_id(root: &str, job: &BackgroundJob) -> String {
    let json = serde_json::to_vec(job).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(root.as_bytes());
    digest.update([0]);
    digest.update(&json);
    digest
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Queues `jobs` under `root`; a job already queued is kept once, at its
/// first queue time.
pub async fn enqueue(config: &Config, root: &Namespace, jobs: Vec<BackgroundJob>) {
    if jobs.is_empty() {
        return;
    }
    let _guard = LOCK.lock().await;
    let mut queue = read(&config.workspace_dir);
    let root = root.to_string();
    let mut added = 0;
    for job in jobs {
        let id = job_id(&root, &job);
        if queue.pending.iter().any(|queued| queued.id == id) {
            continue;
        }
        tracing::debug!(id = %id, job = job.name(), root = %root, "[memory:jobs] queued");
        queue.pending.push(QueuedJob {
            id,
            root: root.clone(),
            job,
            queued_at: Utc::now(),
            attempts: 0,
            last_error: None,
        });
        added += 1;
    }
    if added > 0 {
        write(&config.workspace_dir, &queue);
    }
}

/// Whether `workspace_dir` has queued jobs waiting. A cheap, lock-free read
/// for a host deciding which workspaces are worth a run.
pub fn has_pending(workspace_dir: &Path) -> bool {
    !read(workspace_dir).pending.is_empty()
}

/// The queue as it stands.
pub async fn snapshot(config: &Config) -> JobQueue {
    let _guard = LOCK.lock().await;
    read(&config.workspace_dir)
}

/// Which jobs a run takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Those older than `[memory.recall] build_delay_secs`.
    Due,
    /// Every pending job, now.
    All,
    /// One job by id, now.
    One(String),
}

/// Runs the selected jobs. Returns the runs it recorded. A job that fails is
/// kept with its error and retried on a later run, up to a few attempts; an
/// account-wide refusal does not count as one. The run that gives up says so
/// in its reason.
pub async fn run(config: &Config, selection: Selection) -> MemoryResult<Vec<JobRun>> {
    let bound = engine::resolve(config).engine()?;
    let _guard = LOCK.lock().await;
    let mut queue = read(&config.workspace_dir);
    let cutoff = Utc::now()
        - Duration::seconds(i64::try_from(config.memory.recall.build_delay_secs).unwrap_or(0));
    let (take, keep): (Vec<QueuedJob>, Vec<QueuedJob>) =
        queue
            .pending
            .drain(..)
            .partition(|queued| match &selection {
                Selection::Due => queued.queued_at <= cutoff,
                Selection::All => true,
                Selection::One(id) => &queued.id == id,
            });
    if let Selection::One(id) = &selection {
        if take.is_empty() {
            queue.pending = keep;
            return Err(MemoryError::invalid(format!("no queued job `{id}`")));
        }
    }
    queue.pending = keep;
    let mut runs = Vec::new();
    for mut queued in take {
        let layout = queued
            .root
            .parse::<Namespace>()
            .ok()
            .and_then(|root| MemoryLayout::new(root).ok())
            .unwrap_or_default();
        let runner = BackgroundRunner::new(bound.engine.clone(), layout);
        let name = queued.job.name().to_string();
        let mut record = JobRun {
            id: queued.id.clone(),
            job: name.clone(),
            root: queued.root.clone(),
            ran_at: Utc::now(),
            outcome: String::new(),
            reason: None,
            built: None,
            stored: 0,
        };
        match runner.run(queued.job.clone()).await {
            Ok(report) => {
                record.outcome = match &report.outcome {
                    JobOutcome::Done => "done",
                    JobOutcome::Started => "started",
                    JobOutcome::Scheduled => "scheduled",
                    JobOutcome::Skipped { .. } => "skipped",
                }
                .to_string();
                if let JobOutcome::Skipped { reason } = &report.outcome {
                    record.reason = Some(reason.clone());
                }
                record.built = report
                    .consolidation
                    .as_ref()
                    .and_then(|receipt| receipt.built);
                record.stored = report.stored.len();
                for follow_up in report.follow_ups {
                    let id = job_id(&queued.root, &follow_up);
                    if !queue.pending.iter().any(|pending| pending.id == id) {
                        queue.pending.push(QueuedJob {
                            id,
                            root: queued.root.clone(),
                            job: follow_up,
                            queued_at: Utc::now(),
                            attempts: 0,
                            last_error: None,
                        });
                    }
                }
                tracing::info!(
                    id = %queued.id,
                    job = %name,
                    outcome = %record.outcome,
                    built = ?record.built,
                    "[memory:jobs] ran"
                );
            }
            Err(error) => {
                // A refusal of the whole account (no credits, a rejected
                // credential, an unreachable engine) says nothing about the
                // job, so it keeps its attempts and waits: dropping it would
                // leave a permanent gap in the beliefs (#6718).
                let account_wide = MemoryError::from(error.clone()).is_account_wide();
                if !account_wide {
                    queued.attempts += 1;
                }
                queued.last_error = Some(error.to_string());
                record.outcome = "failed".to_string();
                tracing::warn!(
                    id = %queued.id,
                    job = %name,
                    attempts = queued.attempts,
                    account_wide,
                    %error,
                    "[memory:jobs] run failed"
                );
                if queued.attempts < MAX_ATTEMPTS {
                    record.reason = Some(error.to_string());
                    queue.pending.push(queued);
                } else {
                    record.reason = Some(format!(
                        "dropped after {MAX_ATTEMPTS} failed attempts: {error}"
                    ));
                }
            }
        }
        runs.push(record);
    }
    let mut history = runs.clone();
    history.reverse();
    history.append(&mut queue.history);
    history.truncate(MAX_HISTORY);
    queue.history = history;
    write(&config.workspace_dir, &queue);
    Ok(runs)
}

/// The cron entry point: runs due jobs unless background work is paused.
pub async fn run_due(config: &Config) {
    if let crate::cron::scheduler_gate::Policy::Paused { reason } =
        crate::cron::scheduler_gate::current_policy()
    {
        tracing::debug!(
            ?reason,
            "[memory:jobs] background paused; queue left for later"
        );
        return;
    }
    match run(config, Selection::Due).await {
        Ok(runs) if !runs.is_empty() => {
            tracing::debug!(ran = runs.len(), "[memory:jobs] background run finished");
        }
        Ok(_) => {}
        Err(MemoryError::Off(_)) => {}
        Err(error) => tracing::warn!(%error, "[memory:jobs] background run failed"),
    }
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
