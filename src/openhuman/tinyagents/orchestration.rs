//! Shared orchestration helpers on the `tinyagents` graph layer (issue #4249).
//!
//! openhuman's control plane historically hand-rolled fan-out
//! ([`futures_util::future::join_all`]) and a bespoke detached-sub-agent registry
//! (raw `tokio` `AbortHandle`s, `watch` status channels, tombstone sets). This
//! module is the shared seam that re-expresses that work on `tinyagents`
//! primitives so the detached-sub-agent control plane routes through one place:
//!
//! - The `graph::orchestration` task primitives ([`TaskStore`],
//!   [`OrchestrationTaskKind`], …) are re-exported here so the detached-sub-agent
//!   control plane gets typed task lifecycle bookkeeping (Pending → Running →
//!   Completed/Failed/Cancelled/…) instead of bespoke status enums + watch
//!   channels + tombstones. The store tracks lifecycle; the caller still owns the
//!   executor (the `tokio` task + cooperative cancel + hard abort).
//!
//! Graph lifecycle events are mirrored onto tracing via the shared
//! [`GraphTracingSink`](crate::openhuman::tinyagents::observability::GraphTracingSink).

// Re-export the tinyagents task-orchestration primitives so the detached
// sub-agent control plane imports lifecycle types from one openhuman path.
pub use tinyagents::graph::orchestration::{
    InMemoryTaskStore, OrchestrationControlOutcome, OrchestrationTaskFilter, OrchestrationTaskKind,
    OrchestrationTaskRecord, OrchestrationTaskResult, OrchestrationTaskSpec,
    OrchestrationTaskStatus, TaskStore,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn task_store_tracks_lifecycle() {
        // Smoke the re-exported orchestration primitives: a task moves
        // Pending → Running → Completed and is readable back by id.
        let store = InMemoryTaskStore::new();
        let spec = OrchestrationTaskSpec::new(
            "task-1",
            OrchestrationTaskKind::SubAgent {
                agent: "researcher".to_string(),
            },
        );
        let rec = store.insert(spec).expect("insert");
        assert_eq!(rec.status, OrchestrationTaskStatus::Pending);

        store.mark_running(rec.task_id()).expect("running");
        let done = store
            .complete(rec.task_id(), OrchestrationTaskResult::text("done"))
            .expect("complete");
        assert_eq!(done.status, OrchestrationTaskStatus::Completed);
        assert_eq!(
            store.get(rec.task_id()).map(|r| r.status),
            Some(OrchestrationTaskStatus::Completed)
        );
    }
}
