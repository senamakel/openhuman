//! Persisting a finished run: delivery, run history, one-shot termination,
//! rescheduling, and the high-frequency agent-job warning.

use super::delivery::deliver_if_configured;
use crate::config::Config;
use crate::cron::{
    record_last_run, record_run, remove_job, reschedule_after_run, runs_closer_than, update_job,
    CronJob, CronJobPatch, JobType, Schedule, TooFrequent, MIN_AGENT_JOB_INTERVAL,
};
use chrono::{DateTime, Utc};

pub(super) async fn persist_job_result(
    config: &Config,
    job: &CronJob,
    mut success: bool,
    output: &str,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
) -> bool {
    let duration_ms = (finished_at - started_at).num_milliseconds();

    if let Err(e) = deliver_if_configured(config, job, output, success).await {
        if job.delivery.best_effort {
            tracing::warn!("Cron delivery failed (best_effort): {e}");
        } else {
            success = false;
            tracing::warn!("Cron delivery failed: {e}");
        }
    }

    let _ = record_run(
        config,
        &job.id,
        started_at,
        finished_at,
        if success { "ok" } else { "error" },
        Some(output),
        duration_ms,
    );

    // A fixed-instant (`Schedule::At`) job is inherently one-shot: its `at` is in
    // the past the moment it runs, so `reschedule_after_run` (which writes
    // next_run = at for an `At` schedule) leaves next_run <= now and the job is
    // re-selected by `due_jobs` on every poll, re-executing forever. Terminate
    // every `At` job after a single run, regardless of `delete_after_run`. Only an
    // auto-delete job that succeeded is removed; everything else is kept disabled
    // so its run history stays inspectable. (Inside this `At` branch
    // `is_one_shot_auto_delete` reduces to `job.delete_after_run`.)
    if matches!(job.schedule, Schedule::At { .. }) {
        if is_one_shot_auto_delete(job) && success {
            if let Err(e) = remove_job(config, &job.id) {
                tracing::warn!("Failed to remove one-shot cron job after success: {e}");
            }
        } else {
            let _ = record_last_run(config, &job.id, finished_at, success, output);
            if let Err(e) = update_job(
                config,
                &job.id,
                CronJobPatch {
                    enabled: Some(false),
                    ..CronJobPatch::default()
                },
            ) {
                tracing::warn!("Failed to disable one-shot cron job: {e}");
            }
        }
        return success;
    }

    if let Err(e) = reschedule_after_run(config, job, success, output) {
        tracing::warn!("Failed to persist scheduler run result: {e}");
    }

    success
}

pub(super) fn is_one_shot_auto_delete(job: &CronJob) -> bool {
    job.delete_after_run && matches!(job.schedule, Schedule::At { .. })
}

/// Why an agent job counts as scheduled more often than every five minutes,
/// if it does. `None` for shell and flow jobs, for one-shot `At` schedules,
/// for anything at or above [`MIN_AGENT_JOB_INTERVAL`], and for an expression
/// that cannot be read (that is not evidence of anything).
///
/// Returns the verdict rather than only logging it, because the verdict is
/// the part worth testing: the `Schedule::Cron` arm used to measure the gap
/// between the next run after now and the next run after now plus one
/// second — the same instant unless a run fell inside that second — so every
/// cron-scheduled agent job warned, and no test could see it because a
/// function that only warns has nothing to assert. The gap is now the
/// shortest one between consecutive runs (`runs_closer_than`), so an
/// irregular expression is judged by its tightest pair and not by whichever
/// pair happens to follow the instant of the check.
pub(super) fn agent_job_too_frequent(job: &CronJob) -> Option<TooFrequent> {
    if !matches!(job.job_type, JobType::Agent) {
        return None;
    }
    runs_closer_than(&job.schedule, Utc::now(), MIN_AGENT_JOB_INTERVAL)
}

/// New agent jobs cannot be created below the floor (`validate_agent_schedule`
/// rejects them), so a job that trips this predates the rule. It keeps
/// running — silently skipping runs would turn its schedule into a lie — and
/// this line is the operator's signal to edit it.
pub(super) fn warn_if_high_frequency_agent_job(job: &CronJob) {
    if let Some(too_frequent) = agent_job_too_frequent(job) {
        tracing::warn!(
            "Cron agent job '{}' is scheduled more frequently than every 5 minutes: it \
             {too_frequent}. It was created before that floor was enforced; edit its \
             schedule to silence this.",
            job.id
        );
    }
}
