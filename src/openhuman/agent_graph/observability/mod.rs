//! Observability — turn graph node transitions into traces + bus events.
//!
//! The engine's [`ProgressSink`](crate::openhuman::agent_graph::graph::ProgressSink)
//! is the seam. [`EventBusSink`] is the production impl: it emits `tracing`
//! spans (grep prefix `[agent_graph]`) and publishes the
//! [`DomainEvent::GraphNodeEntered`] / `GraphNodeCompleted` / `GraphRunPaused`
//! family so any subscriber (UI bridge, logging, metrics) can observe a run
//! without coupling to the engine.
//!
//! [`DomainEvent::GraphNodeEntered`]: crate::core::event_bus::DomainEvent::GraphNodeEntered

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::agent_graph::graph::ProgressSink;

/// [`ProgressSink`] that publishes graph lifecycle to the global event bus.
pub struct EventBusSink {
    graph_name: String,
}

impl EventBusSink {
    /// Build a sink tagged with the running graph's name.
    pub fn new(graph_name: impl Into<String>) -> Self {
        Self {
            graph_name: graph_name.into(),
        }
    }

    /// Emit the run-started event (the executor only emits per-node events, so
    /// callers fire this once before `invoke`).
    pub fn run_started(&self, run_id: &str, entry: &str) {
        tracing::info!(
            run_id,
            graph = %self.graph_name,
            entry,
            "[agent_graph] run started"
        );
        publish_global(DomainEvent::GraphRunStarted {
            run_id: run_id.to_string(),
            graph_name: self.graph_name.clone(),
            entry: entry.to_string(),
        });
    }

    /// Emit run-completed.
    pub fn run_completed(&self, run_id: &str, steps: u32, last_node: &str) {
        publish_global(DomainEvent::GraphRunCompleted {
            run_id: run_id.to_string(),
            graph_name: self.graph_name.clone(),
            steps,
            last_node: last_node.to_string(),
        });
    }

    /// Emit run-failed.
    pub fn run_failed(&self, run_id: &str, error: &str) {
        tracing::error!(
            run_id,
            graph = %self.graph_name,
            error,
            "[agent_graph] run failed"
        );
        publish_global(DomainEvent::GraphRunFailed {
            run_id: run_id.to_string(),
            graph_name: self.graph_name.clone(),
            error: error.to_string(),
        });
    }

    /// Emit `GraphRunPaused` (HITL). Called by the runner now that the
    /// tinyagents engine surfaces interrupts as a return value rather than
    /// emitting the openhuman event itself. Inherent wrapper over the
    /// [`ProgressSink`] trait method so callers don't need the trait in scope.
    pub fn emit_paused(&self, run_id: &str, node: &str, kind: &str) {
        ProgressSink::run_paused(self, run_id, node, kind);
    }
}

impl ProgressSink for EventBusSink {
    fn node_entered(&self, run_id: &str, step: u32, node: &str) {
        tracing::debug!(run_id, graph = %self.graph_name, step, node, "[agent_graph] node entered");
        publish_global(DomainEvent::GraphNodeEntered {
            run_id: run_id.to_string(),
            graph_name: self.graph_name.clone(),
            step,
            node: node.to_string(),
        });
    }

    fn node_completed(&self, run_id: &str, step: u32, node: &str, command: &str, elapsed_ms: u64) {
        publish_global(DomainEvent::GraphNodeCompleted {
            run_id: run_id.to_string(),
            graph_name: self.graph_name.clone(),
            step,
            node: node.to_string(),
            command: command.to_string(),
            elapsed_ms,
        });
    }

    fn run_paused(&self, run_id: &str, node: &str, kind: &str) {
        tracing::info!(run_id, graph = %self.graph_name, node, kind, "[agent_graph] run paused (HITL)");
        publish_global(DomainEvent::GraphRunPaused {
            run_id: run_id.to_string(),
            graph_name: self.graph_name.clone(),
            node: node.to_string(),
            kind: kind.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event_bus;

    #[tokio::test]
    async fn sink_publishes_node_and_pause_events() {
        // Fresh bus for the test (idempotent init).
        let _ = event_bus::init_global(64);
        let mut rx = event_bus::global().unwrap().raw_receiver();
        let sink = EventBusSink::new("demo");

        sink.run_started("r1", "start");
        sink.node_entered("r1", 0, "start");
        sink.node_completed("r1", 0, "start", "continue", 2);
        sink.run_paused("r1", "hitl", "approval");
        sink.run_completed("r1", 3, "finalize");

        // Drain and confirm our variants showed up (other tests may share the
        // global bus; filter by run_id).
        let mut names = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            if ev.domain() == "agent_graph" {
                names.push(ev.variant_name());
            }
        }
        assert!(names.contains(&"GraphRunStarted"));
        assert!(names.contains(&"GraphNodeEntered"));
        assert!(names.contains(&"GraphNodeCompleted"));
        assert!(names.contains(&"GraphRunPaused"));
        assert!(names.contains(&"GraphRunCompleted"));
    }
}
