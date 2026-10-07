//! Cron persistence: a thin host wrapper over `tinyflows_sqlite::schedule`.
//!
//! The SQLite job store and run history live upstream (schema, CRUD, output
//! truncation, pruning). This module only turns the host [`Config`] into the
//! store's [`CronStoreOptions`] — database path under the workspace, run-history
//! cap, and due-job batch size — so callers keep passing `&Config`.

use crate::config::Config;
use anyhow::Result;
use chrono::{DateTime, Utc};
use tinyflows_schedule::DeliveryStatus;
use tinyflows_schedule::{CronJob, CronJobPatch, CronRun, DeliveryConfig, Schedule, SessionTarget};
use tinyflows_sqlite::schedule::{self as upstream, AgentJobSpec, CronStoreOptions};

/// Builds the store options from the host config: `<workspace>/cron/jobs.db`,
/// `cron.max_run_history`, `scheduler.max_tasks`.
fn opts(config: &Config) -> CronStoreOptions {
    CronStoreOptions {
        db_path: config.workspace_dir.join("cron").join("jobs.db"),
        max_run_history: config.cron.max_run_history,
        max_tasks: config.scheduler.max_tasks,
    }
}

pub fn add_job(config: &Config, expression: &str, command: &str) -> Result<CronJob> {
    upstream::add_job(&opts(config), expression, command)
}

pub fn add_shell_job(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    command: &str,
) -> Result<CronJob> {
    upstream::add_shell_job(&opts(config), name, schedule, command)
}

#[allow(clippy::too_many_arguments)]
pub fn add_agent_job(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    prompt: &str,
    session_target: SessionTarget,
    model: Option<String>,
    delivery: Option<DeliveryConfig>,
    delete_after_run: bool,
) -> Result<CronJob> {
    upstream::add_agent_job(
        &opts(config),
        name,
        schedule,
        prompt,
        session_target,
        model,
        delivery,
        delete_after_run,
    )
}

/// Like [`add_agent_job`] but accepts an optional built-in agent definition
/// ID and the initial enabled state.
#[allow(clippy::too_many_arguments)]
pub fn add_agent_job_with_definition(
    config: &Config,
    name: Option<String>,
    schedule: Schedule,
    prompt: &str,
    session_target: SessionTarget,
    model: Option<String>,
    delivery: Option<DeliveryConfig>,
    delete_after_run: bool,
    agent_id: Option<String>,
    enabled: bool,
) -> Result<CronJob> {
    upstream::add_agent_job_with_definition(
        &opts(config),
        name,
        schedule,
        prompt,
        session_target,
        model,
        delivery,
        delete_after_run,
        agent_id,
        enabled,
    )
}

/// Adds an agent job described by `spec`, including its origin conversation.
pub fn add_agent_job_from_spec(config: &Config, spec: AgentJobSpec) -> Result<CronJob> {
    upstream::add_agent_job_from_spec(&opts(config), spec)
}

/// Registers (idempotently) the cron job that fires a flow's `schedule`
/// trigger; see `tinyflows_sqlite::schedule::add_flow_schedule_job`.
pub fn add_flow_schedule_job(
    config: &Config,
    flow_id: &str,
    schedule: Schedule,
) -> Result<CronJob> {
    upstream::add_flow_schedule_job(&opts(config), flow_id, schedule)
}

pub fn find_flow_schedule_job(config: &Config, flow_id: &str) -> Result<Option<CronJob>> {
    upstream::find_flow_schedule_job(&opts(config), flow_id)
}

pub fn list_jobs(config: &Config) -> Result<Vec<CronJob>> {
    upstream::list_jobs(&opts(config))
}

pub fn get_job(config: &Config, job_id: &str) -> Result<CronJob> {
    upstream::get_job(&opts(config), job_id)
}

pub fn remove_job(config: &Config, id: &str) -> Result<()> {
    upstream::remove_job(&opts(config), id)?;
    if let Err(error) = super::policy::clear_policy(config, id) {
        tracing::warn!(job_id = id, %error, "[cron:store] removing job policy failed");
    }
    println!("✅ Removed cron job {id}");
    Ok(())
}

/// Deletes every cron job in the workspace (E2E `openhuman.test_reset`).
pub fn clear_all_jobs(config: &Config) -> Result<usize> {
    let removed = upstream::clear_all_jobs(&opts(config))?;
    if let Err(error) = super::policy::clear_all_policies(config) {
        tracing::warn!(%error, "[cron:store] clearing job policies failed");
    }
    Ok(removed)
}

/// Removes duplicate jobs sharing a `name`, keeping the one with most history.
pub fn dedup_named_jobs(config: &Config) -> Result<usize> {
    upstream::dedup_named_jobs(&opts(config))
}

pub fn due_jobs(config: &Config, now: DateTime<Utc>) -> Result<Vec<CronJob>> {
    upstream::due_jobs(&opts(config), now)
}

pub fn update_job(config: &Config, job_id: &str, patch: CronJobPatch) -> Result<CronJob> {
    upstream::update_job(&opts(config), job_id, patch)
}

pub fn record_last_run(
    config: &Config,
    job_id: &str,
    finished_at: DateTime<Utc>,
    success: bool,
    output: &str,
) -> Result<()> {
    upstream::record_last_run(&opts(config), job_id, finished_at, success, output)
}

pub fn reschedule_after_run(
    config: &Config,
    job: &CronJob,
    success: bool,
    output: &str,
) -> Result<()> {
    upstream::reschedule_after_run(&opts(config), job, success, output)
}

pub fn record_run(
    config: &Config,
    job_id: &str,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    status: &str,
    output: Option<&str>,
    duration_ms: i64,
) -> Result<()> {
    upstream::record_run(
        &opts(config),
        job_id,
        started_at,
        finished_at,
        status,
        output,
        duration_ms,
    )
}

/// [`record_run`] plus the outcome of delivering the run's result.
#[allow(clippy::too_many_arguments)]
pub fn record_run_with_delivery(
    config: &Config,
    job_id: &str,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    status: &str,
    output: Option<&str>,
    duration_ms: i64,
    delivery_status: Option<DeliveryStatus>,
) -> Result<()> {
    upstream::record_run_with_delivery(
        &opts(config),
        job_id,
        started_at,
        finished_at,
        status,
        output,
        duration_ms,
        delivery_status,
    )
}

/// Removes "queued" placeholder rows so only the real result row remains.
pub fn delete_queued_runs(config: &Config, job_id: &str) -> Result<usize> {
    upstream::delete_queued_runs(&opts(config), job_id)
}

pub fn list_runs(config: &Config, job_id: &str, limit: usize) -> Result<Vec<CronRun>> {
    upstream::list_runs(&opts(config), job_id, limit)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
