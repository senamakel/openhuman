//! Shared streaming-delta forwarder.
//!
//! Both the channel loop (`run_tool_call_loop`) and the stateful `Agent::turn`
//! loop translate the provider's [`ProviderDelta`] SSE stream into
//! [`AgentProgress`] delta events for the web bridge. The two had byte-identical
//! forwarder tasks; [`spawn_delta_forwarder`] is the single copy.

use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::inference::provider::ProviderDelta;

/// Spawn a task that forwards `ProviderDelta`s from the provider's streaming
/// channel into `on_progress` as `AgentProgress` delta events, tagged with
/// `iteration` (1-based). Returns the sender to hand to
/// [`crate::openhuman::inference::provider::ChatRequest::stream`] and the task
/// handle to await after the chat call.
///
/// Returns `(None, None)` when there is no progress sink — the caller then
/// passes `stream: None` and the provider uses its non-streaming HTTP path.
///
/// Backpressure discipline: the forwarder `.await`s each `send`, so streamed
/// deltas arrive in order and are never silently dropped when the downstream
/// bridge is slow. It exits cleanly once the sender is dropped (after the chat
/// call) or the downstream closes.
pub(crate) fn spawn_delta_forwarder(
    on_progress: Option<tokio::sync::mpsc::Sender<AgentProgress>>,
    iteration: u32,
) -> (
    Option<tokio::sync::mpsc::Sender<ProviderDelta>>,
    Option<tokio::task::JoinHandle<()>>,
) {
    let Some(progress_sink) = on_progress else {
        return (None, None);
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ProviderDelta>(128);
    let forwarder = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let mapped = match event {
                ProviderDelta::TextDelta { delta } => AgentProgress::TextDelta { delta, iteration },
                ProviderDelta::ThinkingDelta { delta } => {
                    AgentProgress::ThinkingDelta { delta, iteration }
                }
                ProviderDelta::ToolCallStart { call_id, tool_name } => {
                    AgentProgress::ToolCallArgsDelta {
                        call_id,
                        tool_name,
                        delta: String::new(),
                        iteration,
                    }
                }
                ProviderDelta::ToolCallArgsDelta { call_id, delta } => {
                    AgentProgress::ToolCallArgsDelta {
                        call_id,
                        tool_name: String::new(),
                        delta,
                        iteration,
                    }
                }
            };
            if progress_sink.send(mapped).await.is_err() {
                break;
            }
        }
    });
    (Some(tx), Some(forwarder))
}
