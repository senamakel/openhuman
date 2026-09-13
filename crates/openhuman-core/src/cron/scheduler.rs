//! Background scheduler loop for cron jobs: polls the store for due jobs,
//! runs them with bounded concurrency, persists results, and emits health
//! signals. Job-type execution, retry, delivery, and persistence live in the
//! submodules below.

mod agent_run;
mod delivery;
mod failure_classification;
mod retry;
mod run_record;
mod shell_job;

#[cfg(test)]
#[path = "scheduler_tests.rs"]
mod tests;

#[allow(unused_imports)]
use agent_run::*;
#[allow(unused_imports)]
use delivery::*;
#[allow(unused_imports)]
use failure_classification::*;
#[allow(unused_imports)]
use retry::*;
#[allow(unused_imports)]
use run_record::*;
#[allow(unused_imports)]
use shell_job::*;

pub use delivery::deliver_job;
pub use retry::execute_job_now;

use crate::config::Config;
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::cron::{due_jobs, CronJob};
use crate::security::SecurityPolicy;
use anyhow::Result;
use chrono::Utc;
use futures_util::{stream, StreamExt};
use std::sync::Arc;
use tokio::time::{self, Duration};

const MIN_POLL_SECONDS: u64 = 5;

pub async fn run(config: Config) -> Result<()> {
    // Ensure the global event bus is initialized so cron delivery events
    // are not silently dropped. This is a no-op if already initialized.
    crate::core::bus::init().await.expect("bus init");
    crate::platform::health::bus::register_health_subscriber();

    let poll_secs = config.reliability.scheduler_poll_secs.max(MIN_POLL_SECONDS);
    let mut interval = time::interval(Duration::from_secs(poll_secs));
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
        &config.action_dir,
    ));

    BUS.publish(DomainEvent::SystemStartup {
        component: "scheduler".to_string(),
    });

    // Track the most recently *emitted* scheduler health so we only
    // publish `HealthChanged` on a state transition. Without this the
    // bus would carry a steady `healthy: true` event every poll
    // interval — typically 30 s, forever — churn for any subscriber
    // that logs / persists / reacts to health events. `None` means
    // "nothing emitted yet for this run", so the first successful tick
    // is treated as a transition and emits.
    let mut last_emitted_health: Option<bool> = None;

    loop {
        interval.tick().await;
        tick_once(&config, &security, &mut last_emitted_health).await;
    }
}

/// Single poll cycle of the scheduler loop, extracted so tests can drive
/// it without owning `tokio::time::interval`.
///
/// Emits a `scheduler` health signal in three cases:
/// - Poll itself failed (DB read) → `healthy: false` with the DB error.
/// - Poll succeeded, queue empty or not → `healthy: true` (#3312
///   recovery signal). Without this, a single transient job failure
///   that flipped the component to `error` via [`process_due_jobs`]
///   would stay there indefinitely while the queue was idle — no later
///   event would clear it, the health endpoint would keep returning
///   503, and Docker would mark the container `unhealthy` for hours
///   until a manual restart. Tick-level "still polling" beats
///   job-level success as the recovery signal because the queue is
///   empty most of the time.
/// - Per-job results (handled inside `process_due_jobs`) continue to
///   flip the component back to `healthy: false` on a failure; the
///   next tick that survives the DB read will re-flip it to
///   `healthy: true`, exactly the auto-recovery behaviour the Docker
///   health check needs.
pub(crate) async fn tick_once(
    config: &Config,
    security: &Arc<SecurityPolicy>,
    last_emitted_health: &mut Option<bool>,
) {
    tracing::debug!("[cron:scheduler] tick poll begin");
    let jobs = match due_jobs(config, Utc::now()) {
        Ok(jobs) => jobs,
        Err(e) => {
            tracing::warn!("[cron:scheduler] tick poll db_error: {e}");
            // Transition-only emission: only publish on the first
            // failure after a previous healthy (or unknown) state.
            // Repeat DB failures stay quiet so subscribers don't see
            // an event-storm during a long outage.
            if *last_emitted_health != Some(false) {
                BUS.publish(DomainEvent::HealthChanged {
                    component: "scheduler".to_string(),
                    healthy: false,
                    message: Some(e.to_string()),
                });
                *last_emitted_health = Some(false);
            }
            return;
        }
    };

    let due_count = jobs.len();
    // Transition-only emission for the recovery / healthy signal: a
    // long idle stretch with no transitions stays silent on the bus,
    // so subscribers don't pay per-poll work for a steady `healthy:
    // true` event every poll interval — the nit oxoxDev caught on
    // #3329. The very first successful tick after boot (or after a
    // failure) is the one that fires; subsequent successful ticks
    // are no-ops on the wire.
    if *last_emitted_health != Some(true) {
        tracing::debug!(
            "[cron:scheduler] tick poll ok due_count={due_count} (recovery signal: healthy=true)"
        );
        BUS.publish(DomainEvent::HealthChanged {
            component: "scheduler".to_string(),
            healthy: true,
            message: None,
        });
        *last_emitted_health = Some(true);
    } else {
        tracing::trace!(
            "[cron:scheduler] tick poll ok due_count={due_count} (steady state, no event)"
        );
    }

    if due_count == 0 {
        tracing::trace!("[cron:scheduler] tick end (no due jobs)");
        return;
    }

    process_due_jobs(config, security, jobs).await;
    tracing::debug!("[cron:scheduler] tick end due_count={due_count} (jobs processed)");

    // `process_due_jobs` itself may have published `healthy: false` on
    // a job failure, but it does so directly on the bus without
    // touching our local tracker. Reset so the next successful tick
    // is again treated as a transition and re-emits `healthy: true` —
    // exactly the auto-recovery behaviour #3312 requires.
    *last_emitted_health = None;
}

async fn process_due_jobs(config: &Config, security: &Arc<SecurityPolicy>, jobs: Vec<CronJob>) {
    let max_concurrent = config.scheduler.max_concurrent.max(1);
    let mut in_flight = stream::iter(jobs.into_iter().map(|job| {
        let config = config.clone();
        let security = Arc::clone(security);
        async move { execute_and_persist_job(&config, security.as_ref(), &job).await }
    }))
    .buffer_unordered(max_concurrent);

    while let Some((job_id, success, failure_message)) = in_flight.next().await {
        if success {
            BUS.publish(DomainEvent::HealthChanged {
                component: "scheduler".to_string(),
                healthy: true,
                message: None,
            });
        } else {
            BUS.publish(DomainEvent::HealthChanged {
                component: "scheduler".to_string(),
                healthy: false,
                message: Some(failure_message.unwrap_or_else(|| format!("job {job_id} failed"))),
            });
        }
    }
}

async fn execute_and_persist_job(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (String, bool, Option<String>) {
    warn_if_high_frequency_agent_job(job);

    let started_at = Utc::now();

    BUS.publish(DomainEvent::CronJobTriggered {
        job_id: job.id.clone(),
        job_name: job.name.clone().unwrap_or_default(),
        job_type: format!("{:?}", job.job_type),
    });

    let (execution_success, output) = execute_job_with_retry(config, security, job).await;
    let finished_at = Utc::now();
    let success = persist_job_result(
        config,
        job,
        execution_success,
        &output,
        started_at,
        finished_at,
    )
    .await;

    BUS.publish(DomainEvent::CronJobCompleted {
        job_id: job.id.clone(),
        success,
        output: crate::util::truncate_with_ellipsis(&output, 512),
    });
    let failure_message = (!success).then(|| crate::util::truncate_with_ellipsis(&output, 256));

    (job.id.clone(), success, failure_message)
}
