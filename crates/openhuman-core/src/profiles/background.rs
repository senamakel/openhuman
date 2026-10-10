//! Background work for profiles.
//!
//! A single-user core drains its memory job queue from a cron system job,
//! gated by the process-wide scheduler gate. Neither fits SaaS: the cron
//! service is off, and the gate reflects the operator (who holds no
//! credential) rather than any user. Instead, one loop per process visits the
//! provisioned profiles:
//!
//! - it closes profiles that have sat idle past `idle_evict_secs`;
//! - for each profile whose workspace has queued memory jobs (deferred ingests,
//!   belief builds), it opens the profile and runs the due jobs **under that
//!   profile's context**, so the jobs read that user's config, credential and
//!   memory root and nobody else's.
//!
//! Users run one after another, and the memory job queue serialises them
//! further; a single slow user delays the rest of a tick, never another
//! user's data.

use std::sync::Arc;
use std::time::Duration;

use super::host::ProfileHost;
use crate::core::runtime::CoreContext;
use crate::memory::lifecycle::jobs::{self, Selection};

/// How often the loop visits the profiles.
pub const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// What one visit did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// Profiles with queued memory jobs that were run.
    pub ran: usize,
    /// Profiles whose jobs could not be run (not openable, engine off, …).
    pub skipped: usize,
}

/// Start the loop. It runs until the process exits.
pub fn spawn(host: Arc<ProfileHost>) -> tokio::task::JoinHandle<()> {
    crate::core::runtime::spawn_scoped(async move {
        log::info!(
            "[profiles][background] started (every {}s)",
            TICK_INTERVAL.as_secs()
        );
        let mut interval = tokio::time::interval(TICK_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            let report = tick(&host).await;
            if report != TickReport::default() {
                log::debug!(
                    "[profiles][background] tick ran={} skipped={}",
                    report.ran,
                    report.skipped
                );
            }
        }
    })
}

/// One visit to every provisioned profile.
pub async fn tick(host: &ProfileHost) -> TickReport {
    host.evict_idle().await;
    let mut report = TickReport::default();
    let profiles = match host.list().await {
        Ok(profiles) => profiles,
        Err(error) => {
            log::warn!("[profiles][background] listing profiles failed: {error}");
            return report;
        }
    };
    for summary in profiles {
        let id = summary.profile_id;
        if !jobs::has_pending(&host.layout_of(&id).workspace_dir) {
            continue;
        }
        let state = match host.open(&id).await {
            Ok(state) => state,
            Err(error) => {
                log::debug!("[profiles][background] profile={id} not opened: {error}");
                report.skipped += 1;
                continue;
            }
        };
        let config = state.config.clone();
        let result = CoreContext::scope(Arc::clone(state.context()), async move {
            jobs::run(&config, Selection::Due).await
        })
        .await;
        match result {
            Ok(runs) => {
                log::debug!(
                    "[profiles][background] profile={id} ran {} memory job(s)",
                    runs.len()
                );
                report.ran += 1;
            }
            Err(error) => {
                log::debug!("[profiles][background] profile={id} memory jobs not run: {error}");
                report.skipped += 1;
            }
        }
    }
    report
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;
