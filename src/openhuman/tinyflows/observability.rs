//! Host [`tinyflows::observability::RunObserver`] impls for the `flows::`
//! domain.
//!
//! `tinyflows` emits structured run/step records; the host decides what to do
//! with them. Two observers live here:
//!
//! - [`TracingRunObserver`] — log-only, for `run_with_observer` call sites and
//!   as a simple example.
//! - [`FlowRunObserver`] — the real one used by the durable run path (issue
//!   G2, live run observation). It publishes a [`DomainEvent::FlowRunProgress`]
//!   twice per non-trigger node — `running` when the node activates, then its
//!   terminal `success`/`error` when it finishes — so the frontend socket
//!   bridge can stream the run advancing node-by-node. On finish it also
//!   persists a [`FlowRunStep`] incrementally into the run's `flow_runs` row
//!   via `flows::store::upsert_flow_run_step`. All effects are best-effort: a
//!   step that fails to persist is logged, never fatal to the run.
//!
//!   The `running` half matters for more than symmetry: the canvas's
//!   `.flow-node-running` pulse is bound to that status, so while only
//!   `on_step_finish` was implemented the pulse was unreachable and the live
//!   overlay rendered as a completion trail — each node gaining a static ring
//!   only after it had already finished.
//!
//! The durable + journaled run path used by `flows_run`/`flows_resume` now
//! accepts an observer (tinyflows 0.3.1's
//! `run_with_checkpointer_journaled_observed` /
//! `resume_with_checkpointer_journaled_observed`), so [`FlowRunObserver`] sees
//! steps live instead of the run being reconstructed only after it settles.

use std::sync::Arc;

use tinyflows::observability::{ExecutionStep, Run, RunObserver, StepStatus};

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::config::Config;
use crate::openhuman::flows::{upsert_flow_run_step, FlowRunStep};

/// Logs run/step lifecycle events with grep-friendly `[flows]` prefixes.
pub struct TracingRunObserver {
    pub run_label: String,
}

impl RunObserver for TracingRunObserver {
    fn on_run_start(&self, run_id: &str) {
        tracing::info!(target: "flows", run_label = %self.run_label, %run_id, "[flows] run start");
    }

    fn on_step_finish(&self, step: &ExecutionStep) {
        tracing::debug!(
            target: "flows",
            run_label = %self.run_label,
            node = %step.node_id,
            status = ?step.status,
            duration_ms = step.duration_ms,
            "[flows] step finished"
        );
    }

    fn on_run_finish(&self, run: &Run) {
        tracing::info!(
            target: "flows",
            run_label = %self.run_label,
            id = %run.id,
            status = ?run.status,
            steps = run.steps.len(),
            "[flows] run finish"
        );
    }
}

/// Maps a live [`StepStatus`] to the stable `"success"`/`"error"` string
/// persisted on [`FlowRunStep::status`] and published on
/// [`DomainEvent::FlowRunProgress`]. `StepStatus` does not derive `PartialEq`,
/// so this matches rather than compares.
fn step_status_str(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Success => "success",
        StepStatus::Error => "error",
    }
}

/// The durable-run observer (issue G2): persists each finished step
/// incrementally and streams a progress event to the frontend.
///
/// Held by the engine as `Arc<dyn RunObserver>` and cloned into node handlers
/// that run across threads, so it must be cheap and non-blocking-ish — it does
/// one small SQLite read-modify-write per step (WAL + `busy_timeout` absorb the
/// concurrent settle write) and one fire-and-forget broadcast publish. Never
/// logs step output/PII: only ids, status, and timing are logged/emitted.
pub struct FlowRunObserver {
    config: Arc<Config>,
    flow_id: String,
    /// The run's checkpointer thread id, which doubles as the `flow_runs` row
    /// id and the `run_id` on the emitted progress event.
    run_id: String,
}

impl FlowRunObserver {
    /// Builds an observer bound to one `flows_run` / `flows_resume` invocation.
    pub fn new(config: Arc<Config>, flow_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            config,
            flow_id: flow_id.into(),
            run_id: run_id.into(),
        }
    }
}

impl FlowRunObserver {
    /// Publishes one item-level progress frame.
    ///
    /// Item events are broadcast only — unlike a step they are never persisted.
    /// The durable `flow_runs` row records what the *node* did, and writing a
    /// row per item would multiply run-history storage by the fan-out width to
    /// record something no run view reads back. Liveness is all these carry, so
    /// a dropped frame costs nothing but a stale spinner, which the UI's
    /// existing poller reconciles.
    fn publish_item(&self, node_id: &str, index: usize, total: usize, status: &str) {
        tracing::debug!(
            target: "flows",
            flow_id = %self.flow_id,
            run_id = %self.run_id,
            node = %node_id,
            index,
            total,
            status,
            "[flows] observer: fanned-out item progress"
        );
        publish_global(DomainEvent::FlowRunItemProgress {
            run_id: self.run_id.clone(),
            node_id: node_id.to_string(),
            index,
            total,
            status: status.to_string(),
        });
    }
}

impl RunObserver for FlowRunObserver {
    fn on_run_start(&self, run_id: &str) {
        tracing::debug!(
            target: "flows",
            flow_id = %self.flow_id,
            run_id = %self.run_id,
            engine_run_id = %run_id,
            "[flows] observer: run start"
        );
    }

    /// Announces a node as `running` the moment it activates, so the canvas can
    /// show what is executing *now* rather than only what has already finished.
    ///
    /// Without this the overlay could never do that: `on_step_finish` is the
    /// only event that ever fired, so the socket only ever carried `success` /
    /// `failed`, and a node stayed unstyled until it was already done. The
    /// canvas's `.flow-node-running` pulse (`flow-node-run-pulse`, written for
    /// exactly this) was therefore unreachable, and the "live run overlay" read
    /// as a completion trail — nodes acquiring a static ring after the fact.
    ///
    /// Publish-only, deliberately: the durable `flow_runs` row stays
    /// finish-driven. A started-but-unfinished node has no output, duration, or
    /// terminal status to record, and writing a placeholder row would put a
    /// `running` step into run history that the post-hoc reconstruction at
    /// settle would then have to reconcile away. The socket event is ephemeral
    /// UI state; the row remains the source of truth.
    fn on_step_start(&self, node_id: &str) {
        tracing::debug!(
            target: "flows",
            flow_id = %self.flow_id,
            run_id = %self.run_id,
            node = %node_id,
            "[flows] observer: step started — publishing FlowRunProgress(running)"
        );
        publish_global(DomainEvent::FlowRunProgress {
            run_id: self.run_id.clone(),
            node_id: node_id.to_string(),
            status: "running".to_string(),
        });
    }

    fn on_item_start(&self, node_id: &str, index: usize, total: usize) {
        self.publish_item(node_id, index, total, "running");
    }

    fn on_item_finish(&self, node_id: &str, index: usize, total: usize, ok: bool) {
        // `ok` is the item's own outcome, independent of whether it went on to
        // fail the node — under the default `collect` policy a failed item is
        // reported here and the step still succeeds.
        self.publish_item(node_id, index, total, if ok { "success" } else { "error" });
    }

    fn on_step_finish(&self, step: &ExecutionStep) {
        let status = step_status_str(&step.status);
        tracing::debug!(
            target: "flows",
            flow_id = %self.flow_id,
            run_id = %self.run_id,
            node = %step.node_id,
            status,
            duration_ms = step.duration_ms,
            "[flows] observer: step finished — persisting incrementally"
        );

        // Persist the live step. `duration_ms` is `u128` on the engine side;
        // clamp into `u64` (a node executor taking > 584 million years is not a
        // real concern, but never panic on cast).
        if !step.diagnostics.is_empty() {
            tracing::warn!(
                target: "flows",
                flow_id = %self.flow_id,
                run_id = %self.run_id,
                node = %step.node_id,
                diagnostics = ?step.diagnostics,
                "[flows] observer: step reported null-resolved expression(s) — persisting for the run view"
            );
        }
        let flow_step = FlowRunStep {
            node_id: step.node_id.clone(),
            output: step.output.clone(),
            port: None,
            status: Some(status.to_string()),
            duration_ms: Some(u64::try_from(step.duration_ms).unwrap_or(u64::MAX)),
            diagnostics: step
                .diagnostics
                .iter()
                .map(|d| serde_json::to_value(d).unwrap_or(serde_json::Value::Null))
                .collect(),
        };
        if let Err(e) = upsert_flow_run_step(&self.config, &self.run_id, &flow_step) {
            tracing::warn!(
                target: "flows",
                flow_id = %self.flow_id,
                run_id = %self.run_id,
                node = %step.node_id,
                error = %e,
                "[flows] observer: failed to persist incremental step (run continues; post-hoc reconstruction will fill it in at settle)"
            );
        }

        // Best-effort progress feed to the frontend socket bridge. The durable
        // `flow_runs` row remains the source of truth; this event may be
        // dropped under broadcast lag, so the UI keeps its 2s poller fallback.
        tracing::debug!(
            target: "flows",
            flow_id = %self.flow_id,
            run_id = %self.run_id,
            node = %step.node_id,
            status,
            "[flows] observer: publishing FlowRunProgress"
        );
        publish_global(DomainEvent::FlowRunProgress {
            run_id: self.run_id.clone(),
            node_id: step.node_id.clone(),
            status: status.to_string(),
        });
    }

    fn on_run_finish(&self, run: &Run) {
        tracing::debug!(
            target: "flows",
            flow_id = %self.flow_id,
            run_id = %self.run_id,
            engine_run_id = %run.id,
            steps = run.steps.len(),
            status = ?run.status,
            "[flows] observer: run finished"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tinyflows::observability::RunStatus;

    #[test]
    fn callbacks_do_not_panic() {
        let observer = TracingRunObserver {
            run_label: "test".to_string(),
        };
        observer.on_run_start("run-1");
        observer.on_step_finish(&ExecutionStep {
            node_id: "n".to_string(),
            status: StepStatus::Success,
            output: Value::Null,
            duration_ms: 5,
            diagnostics: Vec::new(),
        });
        observer.on_run_finish(&Run {
            id: "run-1".to_string(),
            status: RunStatus::Completed,
            steps: Vec::new(),
        });
    }

    #[test]
    fn step_status_maps_to_stable_strings() {
        assert_eq!(step_status_str(&StepStatus::Success), "success");
        assert_eq!(step_status_str(&StepStatus::Error), "error");
    }

    /// The canvas's `.flow-node-running` pulse is bound to a `running` status
    /// that only `on_step_start` can produce. Before it was implemented the
    /// socket carried nothing but `success`/`failed`, so the pulse was
    /// unreachable and the "live" overlay was really a completion trail.
    /// Assert the trait method is actually overridden — a default no-op here
    /// silently returns the UI to that state with every other test still green.
    #[tokio::test]
    async fn item_callbacks_publish_running_then_terminal_for_each_item() {
        // A fanned-out node is one step, so without these frames a canvas can
        // only show the node as busy — never how far through the batch it is.
        use crate::core::event_bus::{
            init_global, subscribe_global, EventHandler, DEFAULT_CAPACITY,
        };
        use async_trait::async_trait;
        use std::sync::Mutex as StdMutex;

        type Frame = (String, usize, usize, String);

        struct Collector {
            run_id: String,
            frames: Arc<StdMutex<Vec<Frame>>>,
        }

        #[async_trait]
        impl EventHandler for Collector {
            fn name(&self) -> &str {
                "test::flows::observability::item_progress_collector"
            }
            fn domains(&self) -> Option<&[&str]> {
                Some(&["cron"])
            }
            async fn handle(&self, event: &DomainEvent) {
                if let DomainEvent::FlowRunItemProgress {
                    run_id,
                    node_id,
                    index,
                    total,
                    status,
                } = event
                {
                    // The bus is process-global and shared with concurrent
                    // tests, so keep only this run's frames.
                    if run_id == &self.run_id {
                        self.frames.lock().unwrap().push((
                            node_id.clone(),
                            *index,
                            *total,
                            status.clone(),
                        ));
                    }
                }
            }
        }

        init_global(DEFAULT_CAPACITY);
        let run_id = "run-item-progress-test";
        let frames: Arc<StdMutex<Vec<Frame>>> = Arc::new(StdMutex::new(Vec::new()));
        let _handle = subscribe_global(Arc::new(Collector {
            run_id: run_id.to_string(),
            frames: Arc::clone(&frames),
        }))
        .expect("bus subscriber installed");

        let observer = FlowRunObserver::new(Arc::new(Config::default()), "flow-1", run_id);
        observer.on_item_start("work", 2, 5);
        observer.on_item_finish("work", 2, 5, true);
        observer.on_item_finish("work", 3, 5, false);

        // Delivery is async; wait for the three frames rather than racing them.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while frames.lock().unwrap().len() < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("three item frames published");

        assert_eq!(
            *frames.lock().unwrap(),
            vec![
                ("work".to_string(), 2, 5, "running".to_string()),
                ("work".to_string(), 2, 5, "success".to_string()),
                // A failed item is still reported: under the default `collect`
                // policy the step succeeds, so this frame is the only signal
                // that one of the workers went wrong.
                ("work".to_string(), 3, 5, "error".to_string()),
            ]
        );
    }

    #[test]
    fn on_step_start_is_implemented_so_the_running_status_can_reach_the_canvas() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        // A probe observer whose `on_step_start` records that the *trait's*
        // dispatch reached an override rather than the blanket default.
        struct Probe {
            started: Arc<AtomicBool>,
        }
        impl RunObserver for Probe {
            fn on_step_start(&self, _node_id: &str) {
                self.started.store(true, Ordering::SeqCst);
            }
        }
        let started = Arc::new(AtomicBool::new(false));
        let probe: Box<dyn RunObserver> = Box::new(Probe {
            started: started.clone(),
        });
        probe.on_step_start("n1");
        assert!(
            started.load(Ordering::SeqCst),
            "the engine dispatches on_step_start through the trait object — if this ever \
             stops holding, FlowRunObserver's override cannot fire either"
        );

        // And the real observer must override it, not inherit the no-op.
        let src = include_str!("observability.rs");
        assert!(
            src.contains("fn on_step_start(&self, node_id: &str)"),
            "FlowRunObserver must implement on_step_start — without it no `running` \
             status is ever published and the canvas pulse is dead code"
        );
        assert!(
            src.contains("status: \"running\".to_string()"),
            "on_step_start must publish FlowRunProgress with status=running"
        );
    }

    // The end-to-end proof that `FlowRunObserver::on_step_finish` persists each
    // step into the `flow_runs` row lives in `flows::ops_tests`
    // (`observer_persists_each_step_incrementally` and the run-driven
    // `flows_run_persists_live_steps_with_status_and_timing`), where the flows
    // `store` internals are in scope for seeding/asserting rows.
}
