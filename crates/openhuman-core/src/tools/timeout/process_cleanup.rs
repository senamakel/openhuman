//! Track command reaping for an embedder that awaits turn cancellation.

use std::future::Future;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

tokio::task_local! {
    static ACTIVE: Vec<ProcessCleanup>;
}

/// Command waiters belonging to one turn. Clone before scoping the turn;
/// after dropping the turn future, [`wait`](Self::wait) awaits their reaping.
/// Tasks spawned by a host tool must explicitly inherit this scope.
#[derive(Clone, Default)]
pub struct ProcessCleanup(Arc<Mutex<Vec<watch::Receiver<bool>>>>);

impl ProcessCleanup {
    /// Whether the current task must acknowledge owned subprocess cleanup.
    /// Interpreter pools have no per-job cancellation acknowledgement and
    /// therefore cannot accept work from this scope.
    pub fn is_active() -> bool {
        ACTIVE.try_with(|_| ()).is_ok()
    }

    /// Run a future with command waiters registered to this turn.
    pub async fn scope<T>(&self, future: impl Future<Output = T>) -> T {
        let mut scopes = ACTIVE.try_with(Clone::clone).unwrap_or_default();
        scopes.push(self.clone());
        ACTIVE.scope(scopes, future).await
    }

    /// Wait for every registered command to exit and its output pipes to close.
    /// Call only after the scoped future has finished or been dropped, so no
    /// further commands can be registered by it.
    pub async fn wait(&self) {
        let waiters = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for mut waiter in waiters {
            let _ = waiter.wait_for(|done| *done).await;
        }
    }
}

pub(super) struct Reaped(watch::Sender<bool>);

impl Reaped {
    pub(super) fn register() -> Self {
        let (done, waiter) = watch::channel(false);
        let _ = ACTIVE.try_with(|scopes| {
            for cleanup in scopes {
                cleanup
                    .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(waiter.clone());
            }
        });
        Self(done)
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}
