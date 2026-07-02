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
//! - [`SteeringRegistry`] is re-exported as the next bridge for task-id-addressed
//!   steering. Today the live control path still goes through OpenHuman's
//!   `RunQueue`; the registry gives the follow-up patch one local import seam for
//!   registering the TinyAgents [`SteeringHandle`] per detached task.
//!
//! Graph lifecycle events are mirrored onto tracing via the shared
//! [`GraphTracingSink`](crate::openhuman::tinyagents::observability::GraphTracingSink).

use std::sync::OnceLock;

// Re-export the tinyagents task-orchestration primitives so the detached
// sub-agent control plane imports lifecycle types from one openhuman path.
#[cfg(test)]
pub(crate) use tinyagents::graph::orchestration::OrchestrationTaskStatus;
#[allow(unused_imports)]
pub(crate) use tinyagents::graph::orchestration::SteeringRegistry;
pub(crate) use tinyagents::graph::orchestration::{
    InMemoryTaskStore, JsonlTaskStore, OrchestrationControlOutcome, OrchestrationTaskFilter,
    OrchestrationTaskKind, OrchestrationTaskRecord, OrchestrationTaskResult, OrchestrationTaskSpec,
    TaskStore,
};
#[allow(unused_imports)]
pub(crate) use tinyagents::harness::ids::TaskId;
#[allow(unused_imports)]
pub(crate) use tinyagents::harness::steering::{SteeringCommand, SteeringHandle};

static STEERING_REGISTRY: OnceLock<SteeringRegistry> = OnceLock::new();

/// Process-local registry for TinyAgents steering handles keyed by detached
/// task id. The current product control path still uses OpenHuman's `RunQueue`;
/// this registry is the crate-native lookup seam for the next control-plane
/// migration slice.
pub(crate) fn shared_steering_registry() -> &'static SteeringRegistry {
    STEERING_REGISTRY.get_or_init(SteeringRegistry::new)
}

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

    #[test]
    fn steering_registry_reexport_registers_task_handles() {
        let registry = shared_steering_registry();
        let handle = SteeringHandle::allow_all();
        let task_id = TaskId::new("task-steer");

        registry.register(task_id.clone(), handle);
        assert!(registry.get(&task_id).is_some());
        assert!(registry.deregister(&task_id).is_some());
        assert!(registry.get(&task_id).is_none());
    }
}
