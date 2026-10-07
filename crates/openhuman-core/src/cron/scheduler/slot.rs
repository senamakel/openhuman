//! The process-wide claim on the cron poll loop.
//!
//! Two loops in one process would each read the same due rows and run every
//! job twice. A runtime that is dropped and rebuilt in the same process (an
//! embedder, a test) must also be able to start a fresh loop once the old one
//! is gone, so the claim is a guard released on drop — including when the
//! loop's task is aborted.

use std::sync::atomic::{AtomicBool, Ordering};

static LIVE: AtomicBool = AtomicBool::new(false);

/// Held by the running poll loop.
#[must_use = "the slot is released when the guard drops"]
pub(crate) struct SchedulerSlot(());

impl SchedulerSlot {
    /// Claim the slot; `None` when another loop holds it.
    pub(crate) fn acquire() -> Option<Self> {
        (!LIVE.swap(true, Ordering::AcqRel)).then(|| {
            tracing::debug!("[cron:scheduler] poll loop slot acquired");
            Self(())
        })
    }
}

impl Drop for SchedulerSlot {
    fn drop(&mut self) {
        LIVE.store(false, Ordering::Release);
        tracing::debug!("[cron:scheduler] poll loop slot released");
    }
}

/// Whether a cron poll loop is running in this process.
pub fn is_running() -> bool {
    LIVE.load(Ordering::Acquire)
}
