//! Bridge the `tinyagents` harness event stream onto openhuman's
//! [`AgentProgress`] + cost tracker (issue #4249, tinyagents 0.2.0).
//!
//! 0.2.0 emits a typed [`AgentEvent`] stream (model started/delta/completed,
//! tool started/completed, usage) through an [`EventSink`] that callers attach
//! to a [`RunContext`]. This listener translates those into the same
//! `AgentProgress` events the legacy `run_turn_engine` produced — restoring the
//! live tool timeline, streaming text, and the cost/token footer on the
//! tinyagents path — and feeds per-call usage into the global cost tracker.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::Sender;

use tinyagents::harness::events::{AgentEvent, EventListener, EventRecord};
use tinyagents::harness::usage::Usage;

use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::inference::provider::UsageInfo;
use crate::openhuman::tools::traits::humanize_tool_name;

#[derive(Default)]
struct BridgeState {
    /// 1-based model-call (iteration) counter.
    iteration: u32,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    charged_amount_usd: f64,
}

/// An [`EventListener`] that mirrors harness events onto openhuman's progress
/// sink and cost tracker.
pub struct OpenhumanEventBridge {
    on_progress: Option<Sender<AgentProgress>>,
    model: String,
    max_iterations: u32,
    state: Mutex<BridgeState>,
}

impl OpenhumanEventBridge {
    /// Build a bridge for `model`, forwarding progress to `on_progress` (when
    /// present) and accumulating usage for the cost footer.
    pub fn new(
        on_progress: Option<Sender<AgentProgress>>,
        model: impl Into<String>,
        max_iterations: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            on_progress,
            model: model.into(),
            max_iterations: max_iterations as u32,
            state: Mutex::new(BridgeState::default()),
        })
    }

    /// Cumulative `(input_tokens, output_tokens, charged_usd)` observed so far.
    pub fn totals(&self) -> (u64, u64, f64) {
        let s = self.state.lock().unwrap();
        (s.input_tokens, s.output_tokens, s.charged_amount_usd)
    }

    /// Best-effort, non-blocking progress emit (drops on a full channel, like
    /// the legacy streaming path).
    fn send(&self, progress: AgentProgress) {
        if let Some(tx) = &self.on_progress {
            let _ = tx.try_send(progress);
        }
    }

    fn iteration(&self) -> u32 {
        self.state.lock().unwrap().iteration
    }

    /// Accumulate a usage block, feed the global cost tracker, and emit a
    /// `TurnCostUpdated` so the UI footer stays live.
    fn record_usage(&self, usage: &Usage) {
        let (iteration, input, output, cached, charged) = {
            let mut s = self.state.lock().unwrap();
            s.input_tokens += usage.input_tokens;
            s.output_tokens += usage.output_tokens;
            s.cached_input_tokens += usage.cache_read_tokens;
            (
                s.iteration,
                s.input_tokens,
                s.output_tokens,
                s.cached_input_tokens,
                s.charged_amount_usd,
            )
        };

        // Feed the authoritative global cost tracker (same call the legacy
        // observer made), so the wallet/cost surfaces stay accurate.
        let usage_info = UsageInfo {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            context_window: 0,
            cached_input_tokens: usage.cache_read_tokens,
            charged_amount_usd: 0.0,
        };
        crate::openhuman::cost::record_provider_usage(&self.model, &usage_info);

        self.send(AgentProgress::TurnCostUpdated {
            model: self.model.clone(),
            iteration,
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: cached,
            total_usd: charged,
        });
    }
}

impl EventListener for OpenhumanEventBridge {
    fn on_event(&self, record: &EventRecord) {
        match &record.event {
            AgentEvent::ModelStarted { .. } => {
                let iteration = {
                    let mut s = self.state.lock().unwrap();
                    s.iteration += 1;
                    s.iteration
                };
                self.send(AgentProgress::IterationStarted {
                    iteration,
                    max_iterations: self.max_iterations,
                });
            }
            AgentEvent::ModelDelta { delta, .. } => {
                if !delta.text.is_empty() {
                    self.send(AgentProgress::TextDelta {
                        delta: delta.text.clone(),
                        iteration: self.iteration(),
                    });
                }
            }
            // `UsageRecorded` carries the authoritative per-call usage and fires
            // exactly once per model call; prefer it over `ModelCompleted`'s
            // optional usage to avoid double counting.
            AgentEvent::UsageRecorded { usage } => self.record_usage(usage),
            AgentEvent::ToolStarted { call_id, tool_name } => {
                self.send(AgentProgress::ToolCallStarted {
                    call_id: call_id.as_str().to_string(),
                    tool_name: tool_name.clone(),
                    arguments: serde_json::Value::Null,
                    iteration: self.iteration(),
                    display_label: Some(humanize_tool_name(tool_name)),
                    display_detail: None,
                });
            }
            AgentEvent::ToolCompleted { call_id, tool_name } => {
                self.send(AgentProgress::ToolCallCompleted {
                    call_id: call_id.as_str().to_string(),
                    tool_name: tool_name.clone(),
                    success: true,
                    output_chars: 0,
                    elapsed_ms: 0,
                    iteration: self.iteration(),
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tinyagents::harness::events::EventSink;

    #[tokio::test]
    async fn bridge_forwards_tool_and_cost_progress() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let bridge = OpenhumanEventBridge::new(Some(tx), "mock-model", 10);
        let sink = EventSink::new();
        sink.subscribe(bridge.clone());

        sink.emit(AgentEvent::ModelStarted {
            call_id: "c1".into(),
            model: "mock-model".to_string(),
        });
        sink.emit(AgentEvent::ToolStarted {
            call_id: "c1".into(),
            tool_name: "echo".to_string(),
        });
        sink.emit(AgentEvent::ToolCompleted {
            call_id: "c1".into(),
            tool_name: "echo".to_string(),
        });
        sink.emit(AgentEvent::UsageRecorded {
            usage: Usage::new(100, 40),
        });

        let mut kinds = Vec::new();
        while let Ok(p) = rx.try_recv() {
            kinds.push(match p {
                AgentProgress::IterationStarted { .. } => "iter",
                AgentProgress::ToolCallStarted { .. } => "tool_start",
                AgentProgress::ToolCallCompleted { .. } => "tool_done",
                AgentProgress::TurnCostUpdated { input_tokens, .. } => {
                    assert_eq!(input_tokens, 100);
                    "cost"
                }
                _ => "other",
            });
        }
        assert!(kinds.contains(&"iter"));
        assert!(kinds.contains(&"tool_start"));
        assert!(kinds.contains(&"tool_done"));
        assert!(kinds.contains(&"cost"));

        let (input, output, _) = bridge.totals();
        assert_eq!((input, output), (100, 40));
    }
}
