//! Node.js pooled execution: whether this host wants it, and how to ask for it.

use std::path::PathBuf;
use std::time::Duration;

use tinyruntime_bus::Language;

use super::{PoolExecOutcome, PoolRunError};
use crate::config::{Config, RuntimePoolConfig};

/// A cancellable dispatch uses the owned-subprocess fallback: the pool has
/// no acknowledged per-job abort seam.
///
/// Whether inline `node` jobs should route through the pool.
///
/// Node defaults **on**: each job runs in its own `worker_thread`, so reuse is
/// safe — a fresh module graph and fresh globals per job.
#[must_use]
pub fn enabled(pool: &RuntimePoolConfig) -> bool {
    if crate::tools::timeout::ProcessCleanup::is_active()
        || crate::tools::timeout::CommandEnvironment::is_active()
    {
        tracing::debug!(
            "[node_exec] cancellable turn uses an owned subprocess instead of the pool"
        );
        return false;
    }
    pool.enabled && pool.node.is_enabled(true)
}

/// Run inline JavaScript on a pooled, warm `node` worker.
///
/// # Errors
///
/// [`PoolRunError`], classified so the caller knows whether falling back to a
/// per-call spawn is safe.
pub async fn run_inline(
    config: &Config,
    code: String,
    cwd: Option<PathBuf>,
    timeout: Option<Duration>,
) -> Result<PoolExecOutcome, PoolRunError> {
    super::run_inline(config, &Language::nodejs(), code, cwd, timeout).await
}
