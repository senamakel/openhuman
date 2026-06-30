//! Channel/CLI agent turns on the tinyagents harness (issue #4249).
//!
//! This is the canonical channel/CLI turn path (the legacy `run_tool_call_loop`
//! is removed). Like the sub-agent route, it reuses the same provider + tools
//! over the bus handler's `Arc`-shared tool sets (`tools_registry:
//! Arc<Vec<Box<dyn Tool>>>` + per-turn extras), so no engine lifetime change is
//! needed. It covers the loop's control-flow seams (iteration cap, circuit
//! breakers, stop hooks, context trimming) and — when the caller supplies an
//! `on_progress` sender — mirrors the harness event stream onto `AgentProgress`
//! (live tool timeline, streaming text deltas, cost/token footer) via the same
//! [`OpenhumanEventBridge`](crate::openhuman::tinyagents::OpenhumanEventBridge)
//! the chat route uses.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc::Sender;

use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::config::{MultimodalConfig, MultimodalFileConfig};
use crate::openhuman::inference::provider::{ChatMessage, Provider};
use crate::openhuman::tinyagents::run_turn_via_tinyagents_shared;
use crate::openhuman::tools::Tool;

/// Drive a channel/CLI turn on the graph engine. Returns the final assistant
/// text. When `on_progress` is `Some`, the run streams and mirrors progress
/// onto `AgentProgress`; pass `None` for a fire-and-forget final-text turn.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_channel_turn_via_graph(
    provider: Arc<dyn Provider>,
    history: &mut Vec<ChatMessage>,
    tools_registry: Arc<Vec<Box<dyn Tool>>>,
    extra_tools: Vec<Box<dyn Tool>>,
    visible_tool_names: Option<&HashSet<String>>,
    model: &str,
    temperature: f64,
    max_iterations: usize,
    multimodal: MultimodalConfig,
    multimodal_files: MultimodalFileConfig,
    on_progress: Option<Sender<AgentProgress>>,
) -> Result<String> {
    let extra_arc = Arc::new(extra_tools);

    // The callable set is the visibility whitelist (empty = every tool visible
    // across the registry + per-turn extras). The runner advertises each via its
    // own `spec()`, deduped by name (extras shadow the registry).
    let allowed = visible_tool_names.cloned().unwrap_or_default();

    // Multimodal prep is not yet wired on the tinyagents path (issue #4249);
    // the params are accepted for signature parity with the legacy loop.
    let _ = (&multimodal, &multimodal_files);

    tracing::info!(
        model,
        max_iterations,
        observed = on_progress.is_some(),
        "[channel:graph] routing channel turn through tinyagents harness"
    );
    let outcome = run_turn_via_tinyagents_shared(
        provider,
        model,
        temperature,
        history.clone(),
        vec![extra_arc, tools_registry],
        allowed,
        max_iterations,
        // Mirror the harness event stream onto AgentProgress when the caller
        // (e.g. channel dispatch) supplied a progress sink.
        on_progress,
        // Top-level (parent) turn — no child-progress attribution.
        None,
        // Channel turns don't resolve a per-turn context window here.
        None,
        // No mid-flight steering on the channel path.
        None,
        // No early-exit pause on the channel path.
        &[],
        // Channels surface the cap as an error (legacy `ErrorCheckpoint`), so no
        // graceful cap pause/summary here.
        false,
    )
    .await?;
    *history = outcome.history;
    Ok(outcome.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::inference::provider::{ChatResponse, ToolCall};
    use crate::openhuman::tools::ToolResult;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct PingTool;
    #[async_trait]
    impl Tool for PingTool {
        fn name(&self) -> &str {
            "ping"
        }
        fn description(&self) -> &str {
            "ping"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _a: serde_json::Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success("pong"))
        }
    }

    struct PingThenDone {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Provider for PingThenDone {
        async fn chat_with_system(
            &self,
            _s: Option<&str>,
            _m: &str,
            _model: &str,
            _t: f64,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn chat(
            &self,
            _r: crate::openhuman::inference::provider::ChatRequest<'_>,
            _model: &str,
            _t: f64,
        ) -> anyhow::Result<ChatResponse> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Ok(ChatResponse {
                    tool_calls: vec![ToolCall {
                        id: "p".to_string(),
                        name: "ping".to_string(),
                        arguments: "{}".to_string(),
                        extra_content: None,
                    }],
                    ..Default::default()
                })
            } else {
                Ok(ChatResponse {
                    text: Some("channel done".to_string()),
                    ..Default::default()
                })
            }
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn channel_turn_runs_through_the_graph() {
        let registry: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![Box::new(PingTool)]);
        let mut history = vec![ChatMessage::user("ping please")];
        let text = run_channel_turn_via_graph(
            Arc::new(PingThenDone {
                calls: AtomicUsize::new(0),
            }),
            &mut history,
            registry,
            vec![],
            None,
            "mock-model",
            0.0,
            10,
            MultimodalConfig::default(),
            MultimodalFileConfig::default(),
            None,
        )
        .await
        .expect("channel graph turn runs");
        assert_eq!(text, "channel done");
        assert!(history.iter().any(|m| m.content.contains("pong")));
    }
}
