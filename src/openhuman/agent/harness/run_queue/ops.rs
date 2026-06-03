//! Thread-safe run queue for active-turn message delivery.
//!
//! [`RunQueue`] holds pending messages sorted into three lanes by
//! [`QueueMode`]. Producers (the web channel) push entries; the engine
//! loop drains steers + collects at iteration boundaries, and the web
//! channel drains followups after the turn completes.

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

use super::types::{QueueCounts, QueueEntry, QueueMode};

/// Thread-safe queue shared between the web channel (producer) and
/// the engine loop (consumer). Wrap in `Arc` for cross-task sharing.
pub struct RunQueue {
    steers: Mutex<VecDeque<QueueEntry>>,
    followups: Mutex<VecDeque<QueueEntry>>,
    collects: Mutex<VecDeque<QueueEntry>>,
    /// Pulsed when a steer is pushed so the engine can wake from a
    /// long provider call if needed (future use — currently the drain
    /// is polled at iteration boundaries).
    steer_notify: Notify,
}

impl RunQueue {
    pub fn new() -> Self {
        Self {
            steers: Mutex::new(VecDeque::new()),
            followups: Mutex::new(VecDeque::new()),
            collects: Mutex::new(VecDeque::new()),
            steer_notify: Notify::new(),
        }
    }

    /// Push a message into the appropriate lane based on its mode.
    ///
    /// `Interrupt`-mode entries should never reach here — the web
    /// channel handles them before touching the queue. Debug-asserts
    /// if one slips through.
    pub async fn push(&self, entry: QueueEntry) {
        tracing::debug!(
            mode = entry.mode.as_str(),
            id = %entry.id,
            thread_id = %entry.thread_id,
            chars = entry.message.len(),
            "[run_queue] enqueued"
        );
        match entry.mode {
            QueueMode::Steer => {
                self.steers.lock().await.push_back(entry);
                self.steer_notify.notify_one();
            }
            QueueMode::Followup => {
                self.followups.lock().await.push_back(entry);
            }
            QueueMode::Collect => {
                self.collects.lock().await.push_back(entry);
            }
            QueueMode::Interrupt => {
                debug_assert!(
                    false,
                    "interrupt-mode entries must not be pushed to RunQueue"
                );
                tracing::warn!(
                    "[run_queue] interrupt entry pushed — dropping id={}",
                    entry.id
                );
            }
        }
    }

    /// Drain all pending steer entries (FIFO). Non-blocking.
    pub async fn drain_steers(&self) -> Vec<QueueEntry> {
        let mut lock = self.steers.lock().await;
        let drained: Vec<_> = lock.drain(..).collect();
        if !drained.is_empty() {
            tracing::debug!(count = drained.len(), "[run_queue] drained steers");
        }
        drained
    }

    /// Drain all pending followup entries (FIFO). Non-blocking.
    pub async fn drain_followups(&self) -> Vec<QueueEntry> {
        let mut lock = self.followups.lock().await;
        let drained: Vec<_> = lock.drain(..).collect();
        if !drained.is_empty() {
            tracing::debug!(count = drained.len(), "[run_queue] drained followups");
        }
        drained
    }

    /// Drain all pending collect entries (FIFO). Non-blocking.
    pub async fn drain_collects(&self) -> Vec<QueueEntry> {
        let mut lock = self.collects.lock().await;
        let drained: Vec<_> = lock.drain(..).collect();
        if !drained.is_empty() {
            tracing::debug!(count = drained.len(), "[run_queue] drained collects");
        }
        drained
    }

    /// Number of pending entries per lane.
    pub async fn pending_counts(&self) -> QueueCounts {
        QueueCounts {
            steers: self.steers.lock().await.len(),
            followups: self.followups.lock().await.len(),
            collects: self.collects.lock().await.len(),
        }
    }

    /// True when all lanes are empty.
    pub async fn is_empty(&self) -> bool {
        self.steers.lock().await.is_empty()
            && self.followups.lock().await.is_empty()
            && self.collects.lock().await.is_empty()
    }

    /// Clear all pending entries across all lanes. Returns the total
    /// number of entries cleared.
    pub async fn clear(&self) -> usize {
        let s = {
            let mut lock = self.steers.lock().await;
            let n = lock.len();
            lock.clear();
            n
        };
        let f = {
            let mut lock = self.followups.lock().await;
            let n = lock.len();
            lock.clear();
            n
        };
        let c = {
            let mut lock = self.collects.lock().await;
            let n = lock.len();
            lock.clear();
            n
        };
        let total = s + f + c;
        if total > 0 {
            tracing::debug!(
                steers = s,
                followups = f,
                collects = c,
                "[run_queue] cleared"
            );
        }
        total
    }

    /// Clear entries for a specific mode only. Returns the count cleared.
    pub async fn clear_mode(&self, mode: QueueMode) -> usize {
        match mode {
            QueueMode::Steer => {
                let mut lock = self.steers.lock().await;
                let n = lock.len();
                lock.clear();
                n
            }
            QueueMode::Followup => {
                let mut lock = self.followups.lock().await;
                let n = lock.len();
                lock.clear();
                n
            }
            QueueMode::Collect => {
                let mut lock = self.collects.lock().await;
                let n = lock.len();
                lock.clear();
                n
            }
            QueueMode::Interrupt => 0,
        }
    }

    /// Get a reference to the steer notify for external wake-up logic.
    pub fn steer_notify(&self) -> &Notify {
        &self.steer_notify
    }
}

impl Default for RunQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RunQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunQueue")
            .field("steers", &"<locked>")
            .field("followups", &"<locked>")
            .field("collects", &"<locked>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_entry(mode: QueueMode, msg: &str) -> QueueEntry {
        QueueEntry {
            id: uuid::Uuid::new_v4().to_string(),
            message: msg.to_string(),
            mode,
            enqueued_at: std::time::Instant::now(),
            client_id: "test-client".to_string(),
            thread_id: "test-thread".to_string(),
        }
    }

    #[tokio::test]
    async fn push_and_drain_steers_fifo() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Steer, "first")).await;
        q.push(make_entry(QueueMode::Steer, "second")).await;
        let drained = q.drain_steers().await;
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].message, "first");
        assert_eq!(drained[1].message, "second");
    }

    #[tokio::test]
    async fn push_and_drain_followups_fifo() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Followup, "a")).await;
        q.push(make_entry(QueueMode::Followup, "b")).await;
        let drained = q.drain_followups().await;
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].message, "a");
        assert_eq!(drained[1].message, "b");
    }

    #[tokio::test]
    async fn push_and_drain_collects_fifo() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Collect, "ctx1")).await;
        q.push(make_entry(QueueMode::Collect, "ctx2")).await;
        let drained = q.drain_collects().await;
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].message, "ctx1");
        assert_eq!(drained[1].message, "ctx2");
    }

    #[tokio::test]
    async fn drain_empties_queue() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Steer, "x")).await;
        assert_eq!(q.drain_steers().await.len(), 1);
        assert!(q.drain_steers().await.is_empty());
    }

    #[tokio::test]
    async fn modes_are_isolated() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Steer, "s")).await;
        q.push(make_entry(QueueMode::Followup, "f")).await;
        q.push(make_entry(QueueMode::Collect, "c")).await;

        assert_eq!(q.drain_steers().await.len(), 1);
        assert_eq!(q.drain_followups().await.len(), 1);
        assert_eq!(q.drain_collects().await.len(), 1);
    }

    #[tokio::test]
    async fn pending_counts_accurate() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Steer, "a")).await;
        q.push(make_entry(QueueMode::Steer, "b")).await;
        q.push(make_entry(QueueMode::Followup, "c")).await;
        q.push(make_entry(QueueMode::Collect, "d")).await;
        q.push(make_entry(QueueMode::Collect, "e")).await;
        q.push(make_entry(QueueMode::Collect, "f")).await;

        let counts = q.pending_counts().await;
        assert_eq!(counts.steers, 2);
        assert_eq!(counts.followups, 1);
        assert_eq!(counts.collects, 3);
    }

    #[tokio::test]
    async fn is_empty_when_all_drained() {
        let q = RunQueue::new();
        assert!(q.is_empty().await);
        q.push(make_entry(QueueMode::Steer, "x")).await;
        assert!(!q.is_empty().await);
        q.drain_steers().await;
        assert!(q.is_empty().await);
    }

    #[tokio::test]
    async fn clear_removes_all() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Steer, "a")).await;
        q.push(make_entry(QueueMode::Followup, "b")).await;
        q.push(make_entry(QueueMode::Collect, "c")).await;
        let cleared = q.clear().await;
        assert_eq!(cleared, 3);
        assert!(q.is_empty().await);
    }

    #[tokio::test]
    async fn clear_mode_selective() {
        let q = RunQueue::new();
        q.push(make_entry(QueueMode::Steer, "a")).await;
        q.push(make_entry(QueueMode::Followup, "b")).await;
        q.push(make_entry(QueueMode::Collect, "c")).await;

        assert_eq!(q.clear_mode(QueueMode::Steer).await, 1);
        assert_eq!(q.pending_counts().await.steers, 0);
        assert_eq!(q.pending_counts().await.followups, 1);
        assert_eq!(q.pending_counts().await.collects, 1);
    }

    #[tokio::test]
    async fn concurrent_push_drain() {
        let q = Arc::new(RunQueue::new());
        let total = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for i in 0..10 {
            let q = q.clone();
            handles.push(tokio::spawn(async move {
                q.push(make_entry(QueueMode::Steer, &format!("msg-{i}")))
                    .await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let drained = q.drain_steers().await;
        total.fetch_add(drained.len(), Ordering::SeqCst);
        assert_eq!(total.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn notify_fires_on_steer_push() {
        let q = Arc::new(RunQueue::new());
        let q2 = q.clone();

        let waiter = tokio::spawn(async move {
            q2.steer_notify().notified().await;
            true
        });

        tokio::task::yield_now().await;
        q.push(make_entry(QueueMode::Steer, "wake")).await;

        let result = tokio::time::timeout(std::time::Duration::from_millis(100), waiter)
            .await
            .expect("notify should fire within timeout")
            .unwrap();
        assert!(result);
    }
}
