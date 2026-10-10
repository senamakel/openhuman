//! An agent's removal state: whether it still accepts turns, how many are in
//! flight, and the teardown of the per-agent state the core keeps for it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{watch, Notify};

use crate::CoreError;

/// Turn admission and in-flight accounting for one agent.
pub(crate) struct Lifecycle {
    removed: watch::Sender<bool>,
    removal_claimed: AtomicBool,
    in_flight: AtomicUsize,
    idle: Notify,
    torn_down: AtomicBool,
}

impl Lifecycle {
    pub(crate) fn new() -> Self {
        Self {
            removed: watch::Sender::new(false),
            removal_claimed: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            idle: Notify::new(),
            torn_down: AtomicBool::new(false),
        }
    }

    /// Stops admitting turns and ends the ones in flight; returns whether this
    /// caller claimed removal before another caller.
    pub(crate) fn mark_removed(&self) -> bool {
        if self.removal_claimed.swap(true, Ordering::SeqCst) {
            return false;
        }
        // A cancelled turn can retain the watch read while its future drops.
        // Only the first claimant writes; repeated teardown never waits on it.
        self.removed.send_replace(true);
        true
    }

    /// Bind handles to this agent instance, even after its public id is reused.
    pub(crate) fn removed(&self) -> watch::Receiver<bool> {
        self.removed.subscribe()
    }

    /// Runs `turn` unless the agent was removed, ending it early with
    /// [`CoreError::AgentRemoved`] if the agent is removed meanwhile.
    pub(crate) async fn admit<T>(
        &self,
        agent_id: &str,
        method: &'static str,
        turn: impl std::future::Future<Output = Result<T, CoreError>>,
    ) -> Result<T, CoreError> {
        let removed = || CoreError::AgentRemoved {
            method,
            agent_id: agent_id.to_string(),
        };
        let mut watcher = self.removed.subscribe();
        if *watcher.borrow_and_update() {
            log::debug!("[embed][agent] turn refused: agent removed id={agent_id}");
            return Err(removed());
        }
        let _in_flight = InFlight::enter(self);
        tokio::select! {
            biased;
            _ = watcher.wait_for(|removed| *removed) => {
                log::debug!("[embed][agent] in-flight turn ended: agent removed id={agent_id}");
                Err(removed())
            }
            outcome = turn => outcome,
        }
    }

    /// Waits until no turn is in flight, for at most `limit`. Returns whether
    /// the agent went idle in time.
    pub(crate) async fn wait_idle(&self, limit: Duration) -> bool {
        let wait = async {
            loop {
                let notified = self.idle.notified();
                if self.in_flight.load(Ordering::SeqCst) == 0 {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(limit, wait).await.is_ok()
    }

    /// Claims the one-time teardown. `false` when it already ran.
    pub(crate) fn begin_teardown(&self) -> bool {
        !self.torn_down.swap(true, Ordering::SeqCst)
    }
}

struct InFlight<'a>(&'a Lifecycle);

impl<'a> InFlight<'a> {
    fn enter(lifecycle: &'a Lifecycle) -> Self {
        lifecycle.in_flight.fetch_add(1, Ordering::SeqCst);
        Self(lifecycle)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if self.0.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
