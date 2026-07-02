//! Shared orchestration helpers on the `tinyagents` graph layer (issue #4249).
//!
//! openhuman's control plane historically hand-rolled fan-out
//! ([`futures_util::future::join_all`]) and a bespoke detached-sub-agent registry
//! (raw `tokio` `AbortHandle`s, `watch` status channels, tombstone sets). This
//! module is the shared seam that re-expresses that work on `tinyagents`
//! primitives so every orchestration surface (workflow phases, parallel agents,
//! teams, detached sub-agents) routes through one place:
//!
//! - [`run_parallel_fanout`] wraps `tinyagents::graph::parallel::map_reduce` so
//!   fan-out callers share the SDK-owned ordered, bounded parallel helper instead
//!   of carrying bespoke dispatch/worker/collect graph machinery.
//! - The `graph::orchestration` task primitives ([`TaskStore`],
//!   [`OrchestrationTaskKind`], …) are re-exported here so the detached-sub-agent
//!   control plane gets typed task lifecycle bookkeeping (Pending → Running →
//!   Completed/Failed/Cancelled/…) instead of bespoke status enums + watch
//!   channels + tombstones. The store tracks lifecycle; the caller still owns the
//!   executor (the `tokio` task + cooperative cancel + hard abort).
//!
//! Graph lifecycle events are mirrored onto tracing via the shared
//! [`GraphTracingSink`](crate::openhuman::tinyagents::observability::GraphTracingSink).

use std::future::Future;
use tinyagents::graph::parallel::{map_reduce, FailurePolicy, ParallelOptions};

// Re-export the tinyagents task-orchestration primitives so the detached
// sub-agent control plane imports lifecycle types from one openhuman path.
pub use tinyagents::graph::orchestration::{
    InMemoryTaskStore, OrchestrationControlOutcome, OrchestrationTaskFilter, OrchestrationTaskKind,
    OrchestrationTaskRecord, OrchestrationTaskResult, OrchestrationTaskSpec,
    OrchestrationTaskStatus, TaskStore,
};

/// Run `run_one(index, item)` for every entry in `items` concurrently through
/// the `tinyagents` ordered map/reduce helper, returning the results in **input
/// order** regardless of completion order. `max_concurrency` bounds how many
/// workers are polled at once.
///
/// This is the generic engine behind the council member fan-out and the
/// `spawn_parallel_agents` tool: pure parallel mechanics with no domain
/// knowledge, so it is unit-testable with a trivial closure.
///
/// `label` names the fan-out for tracing. Each `item` is moved into exactly one
/// worker future (no `Clone` bound on the payload).
pub async fn run_parallel_fanout<I, T, F, Fut>(
    label: &str,
    items: Vec<I>,
    max_concurrency: usize,
    run_one: F,
) -> Result<Vec<T>, String>
where
    I: Send + 'static,
    T: Send + 'static,
    F: Fn(usize, I) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = T> + Send + 'static,
{
    let n = items.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    tracing::debug!(
        target: "orchestration",
        workers = n,
        max_concurrency = max_concurrency.max(1),
        "[orchestration] running parallel fan-out on tinyagents map_reduce ({label})"
    );

    let options = ParallelOptions::default()
        .with_max_concurrency(max_concurrency.max(1))
        .with_failure_policy(FailurePolicy::CollectAll);
    let outcome = map_reduce(items, options, move |i, item| {
        let run_one = run_one.clone();
        async move { Ok(run_one(i, item).await) }
    })
    .await
    .map_err(|e| format!("{label} fan-out map_reduce failed: {e}"))?;

    let mut out = Vec::with_capacity(n);
    for item in outcome.outcomes {
        match item.result {
            Ok(value) => out.push(value),
            Err(err) => {
                return Err(format!(
                    "{label} fan-out: worker {} failed: {err}",
                    item.index
                ));
            }
        }
    }
    if out.len() != n {
        return Err(format!(
            "{label} fan-out: expected {n} result(s), got {}",
            out.len()
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn fanout_runs_every_worker_and_preserves_input_order() {
        let labels = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let ran = Arc::new(AtomicUsize::new(0));
        let ran2 = ran.clone();
        let results = run_parallel_fanout("test", labels, 4, move |i, label| {
            let ran = ran2.clone();
            async move {
                ran.fetch_add(1, Ordering::SeqCst);
                format!("{i}:{label}")
            }
        })
        .await
        .expect("fan-out runs");

        assert_eq!(ran.load(Ordering::SeqCst), 3, "every worker ran once");
        assert_eq!(results, vec!["0:a", "1:b", "2:c"], "results in input order");
    }

    #[tokio::test]
    async fn fanout_empty_is_a_noop() {
        let results =
            run_parallel_fanout::<String, String, _, _>("empty", vec![], 4, |_, _| async move {
                String::new()
            })
            .await
            .expect("empty fan-out runs");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fanout_handles_single_worker() {
        let results =
            run_parallel_fanout("solo", vec!["x".to_string()], 1, |_, label| async move {
                format!("only:{label}")
            })
            .await
            .expect("single-worker fan-out runs");
        assert_eq!(results, vec!["only:x"]);
    }

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
