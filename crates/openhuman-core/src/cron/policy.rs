//! Per-job run policy: retry budget and single-flight.
//!
//! The job row itself (`CronJob`) belongs to `tinyflows-schedule` and its
//! SQLite store to `tinyflows-sqlite`; neither has room for these two knobs,
//! and both are host policy rather than scheduling logic. They live here, in a
//! host-owned `cron_job_policies` table beside the store in the same
//! `<workspace>/cron/jobs.db`, keyed by job id. A job with no row has the
//! default policy, so every existing job keeps its behaviour:
//!
//! - `retries: None` uses `reliability.scheduler_retries`; `Some(0)` means
//!   exactly one attempt.
//! - `single_flight: false` is the scheduler's historical behaviour: a job is
//!   never dispatched twice by the scheduler, and a slot it misses while still
//!   running is skipped silently. `true` additionally records each missed slot
//!   as a `skipped` run and refuses a manual "Run now" while a run is in flight.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// How the scheduler runs one job. The default is the historical behaviour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobPolicy {
    /// Retries after a failed attempt. `None` keeps
    /// `reliability.scheduler_retries`; `Some(0)` runs exactly once.
    pub retries: Option<u32>,
    /// Skip, and record as `skipped`, a run that comes due while the previous
    /// run of the same job is still in flight.
    pub single_flight: bool,
}

pub(crate) fn db_path(config: &Config) -> PathBuf {
    config.workspace_dir.join("cron").join("jobs.db")
}

fn open(config: &Config) -> Result<Connection> {
    let path = db_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cron directory {}", parent.display()))?;
    }
    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open cron store {}", path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cron_job_policies (
             job_id TEXT PRIMARY KEY,
             retries INTEGER,
             single_flight INTEGER NOT NULL DEFAULT 0
         )",
        [],
    )
    .context("Failed to create cron_job_policies")?;
    Ok(conn)
}

/// The policy stored for `job_id`; the default when none is.
pub fn get_policy(config: &Config, job_id: &str) -> Result<JobPolicy> {
    let conn = open(config)?;
    let row = conn
        .query_row(
            "SELECT retries, single_flight FROM cron_job_policies WHERE job_id = ?1",
            params![job_id],
            |row| {
                Ok(JobPolicy {
                    retries: row
                        .get::<_, Option<i64>>(0)?
                        .map(|n| u32::try_from(n.max(0)).unwrap_or(u32::MAX)),
                    single_flight: row.get::<_, i64>(1)? != 0,
                })
            },
        )
        .optional()?;
    Ok(row.unwrap_or_default())
}

/// Store `policy` for `job_id`. The default policy is stored as no row.
pub fn set_policy(config: &Config, job_id: &str, policy: JobPolicy) -> Result<()> {
    if policy == JobPolicy::default() {
        return clear_policy(config, job_id);
    }
    tracing::debug!(job_id, ?policy, "[cron:policy] storing job policy");
    open(config)?.execute(
        "INSERT INTO cron_job_policies (job_id, retries, single_flight) VALUES (?1, ?2, ?3)
         ON CONFLICT(job_id) DO UPDATE SET retries = ?2, single_flight = ?3",
        params![
            job_id,
            policy.retries.map(i64::from),
            i64::from(policy.single_flight)
        ],
    )?;
    Ok(())
}

/// Remove `job_id`'s policy; it reverts to the default.
pub fn clear_policy(config: &Config, job_id: &str) -> Result<()> {
    open(config)?.execute(
        "DELETE FROM cron_job_policies WHERE job_id = ?1",
        params![job_id],
    )?;
    Ok(())
}

/// Remove every stored policy.
pub(crate) fn clear_all_policies(config: &Config) -> Result<usize> {
    Ok(open(config)?.execute("DELETE FROM cron_job_policies", [])?)
}

/// The retry budget for a job with `policy`.
pub fn effective_retries(config: &Config, policy: &JobPolicy) -> u32 {
    policy
        .retries
        .unwrap_or(config.reliability.scheduler_retries)
}

/// The stored policy for `job_id`, or the default when it cannot be read: a
/// policy-store hiccup must not stop a job from running.
pub(crate) fn policy_or_default(config: &Config, job_id: &str) -> JobPolicy {
    get_policy(config, job_id).unwrap_or_else(|error| {
        tracing::warn!(job_id, %error, "[cron:policy] policy unreadable; using the default");
        JobPolicy::default()
    })
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
