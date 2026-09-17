//! Cron persistence: SQLite-backed job storage and run history.
//!
//! Split by responsibility: [`schema`] owns row mapping and the
//! connection/migration setup, [`jobs`] owns job CRUD, and [`runs`] owns
//! run-history recording, output truncation, and history reads.

mod jobs;
mod runs;
mod schema;

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

pub use jobs::{
    add_agent_job, add_agent_job_with_definition, add_flow_schedule_job, add_job, add_shell_job,
    clear_all_jobs, dedup_named_jobs, due_jobs, find_flow_schedule_job, get_job, list_jobs,
    remove_job, update_job,
};
pub use runs::{delete_queued_runs, list_runs, record_last_run, record_run, reschedule_after_run};

// Re-exported (private `use`, visible to this module and its `tests`
// descendant) so `store_tests.rs` / its sub-test-modules can keep exercising
// the connection helper and the `rusqlite::params!` macro directly via
// `use super::*;`, exactly as when this was one spliced file.
#[allow(unused_imports)]
use runs::{MAX_CRON_OUTPUT_BYTES, TRUNCATED_OUTPUT_MARKER};
#[allow(unused_imports)]
use rusqlite::params;
#[allow(unused_imports)]
use schema::with_connection;
