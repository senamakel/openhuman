//! Cron run-history persistence: recording job outcomes with bounded output,
//! pruning old history, and reading it back.

use super::schema::{parse_rfc3339, sql_conversion_error, with_connection};
use crate::config::Config;
use crate::cron::{next_run_for_schedule, CronJob, CronRun};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::params;

pub(crate) const MAX_CRON_OUTPUT_BYTES: usize = 16 * 1024;
pub(crate) const TRUNCATED_OUTPUT_MARKER: &str = "\n...[truncated]";

pub fn record_last_run(
    config: &Config,
    job_id: &str,
    finished_at: DateTime<Utc>,
    success: bool,
    output: &str,
) -> Result<()> {
    let status = if success { "ok" } else { "error" };
    let bounded_output = truncate_cron_output(output);
    with_connection(config, |conn| {
        conn.execute(
            "UPDATE cron_jobs
             SET last_run = ?1, last_status = ?2, last_output = ?3
             WHERE id = ?4",
            params![finished_at.to_rfc3339(), status, bounded_output, job_id],
        )
        .context("Failed to update cron last run fields")?;
        Ok(())
    })
}

pub fn reschedule_after_run(
    config: &Config,
    job: &CronJob,
    success: bool,
    output: &str,
) -> Result<()> {
    let now = Utc::now();
    let next_run = next_run_for_schedule(&job.schedule, now)?;
    let status = if success { "ok" } else { "error" };
    let bounded_output = truncate_cron_output(output);

    with_connection(config, |conn| {
        conn.execute(
            "UPDATE cron_jobs
             SET next_run = ?1, last_run = ?2, last_status = ?3, last_output = ?4
             WHERE id = ?5",
            params![
                next_run.to_rfc3339(),
                now.to_rfc3339(),
                status,
                bounded_output,
                job.id
            ],
        )
        .context("Failed to update cron job run state")?;
        Ok(())
    })
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
    let bounded_output = output.map(truncate_cron_output);
    with_connection(config, |conn| {
        // Wrap INSERT + pruning DELETE in an explicit transaction so that
        // if the DELETE fails, the INSERT is rolled back and the run table
        // cannot grow unboundedly.
        let tx = conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO cron_runs (job_id, started_at, finished_at, status, output, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                job_id,
                started_at.to_rfc3339(),
                finished_at.to_rfc3339(),
                status,
                bounded_output.as_deref(),
                duration_ms,
            ],
        )
        .context("Failed to insert cron run")?;

        let keep = config.cron.max_run_history.max(1) as i64;
        tx.execute(
            "DELETE FROM cron_runs
             WHERE job_id = ?1
               AND id NOT IN (
                 SELECT id FROM cron_runs
                 WHERE job_id = ?1
                 ORDER BY started_at DESC, id DESC
                 LIMIT ?2
               )",
            params![job_id, keep],
        )
        .context("Failed to prune cron run history")?;

        tx.commit()
            .context("Failed to commit cron run transaction")?;
        Ok(())
    })
}

/// Remove all "queued" placeholder records for a given job so that only the
/// real (ok/error) result row remains in the run history.
pub fn delete_queued_runs(config: &Config, job_id: &str) -> Result<usize> {
    with_connection(config, |conn| {
        let deleted = conn.execute(
            "DELETE FROM cron_runs WHERE job_id = ?1 AND status = 'queued'",
            params![job_id],
        )?;
        Ok(deleted)
    })
}

fn truncate_cron_output(output: &str) -> String {
    if output.len() <= MAX_CRON_OUTPUT_BYTES {
        return output.to_string();
    }

    if MAX_CRON_OUTPUT_BYTES <= TRUNCATED_OUTPUT_MARKER.len() {
        return TRUNCATED_OUTPUT_MARKER.to_string();
    }

    let mut cutoff = MAX_CRON_OUTPUT_BYTES - TRUNCATED_OUTPUT_MARKER.len();
    while cutoff > 0 && !output.is_char_boundary(cutoff) {
        cutoff -= 1;
    }

    let mut truncated = output[..cutoff].to_string();
    truncated.push_str(TRUNCATED_OUTPUT_MARKER);
    truncated
}

pub fn list_runs(config: &Config, job_id: &str, limit: usize) -> Result<Vec<CronRun>> {
    with_connection(config, |conn| {
        let lim = i64::try_from(limit.max(1)).context("Run history limit overflow")?;
        let mut stmt = conn.prepare(
            "SELECT id, job_id, started_at, finished_at, status, output, duration_ms
             FROM cron_runs
             WHERE job_id = ?1
             ORDER BY started_at DESC, id DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![job_id, lim], |row| {
            Ok(CronRun {
                id: row.get(0)?,
                job_id: row.get(1)?,
                started_at: parse_rfc3339(&row.get::<_, String>(2)?)
                    .map_err(sql_conversion_error)?,
                finished_at: parse_rfc3339(&row.get::<_, String>(3)?)
                    .map_err(sql_conversion_error)?,
                status: row.get(4)?,
                output: row.get(5)?,
                duration_ms: row.get(6)?,
            })
        })?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    })
}
