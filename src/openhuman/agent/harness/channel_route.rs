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

    // Multimodal prep (parity with the chat route's
    // `run_turn_via_tinyagents_session`, issue #4249): rehydrate image
    // placeholders for vision-capable providers, then expand `[IMAGE:…]` /
    // `[FILE:…]` markers into provider-ready content before dispatch. The
    // expanded copy is provider-only — it is sent to the model but never
    // persisted back into the channel `history` (see the reconstruction below).
    let prior_len = history.len();
    let mut prepared = history.clone();
    if provider.supports_vision()
        && crate::openhuman::agent::multimodal::has_image_placeholders(&prepared)
    {
        prepared = crate::openhuman::agent::multimodal::rehydrate_image_placeholders(&prepared);
    }
    let prepared = crate::openhuman::agent::multimodal::prepare_messages_for_provider(
        &prepared,
        &multimodal,
        &multimodal_files,
    )
    .await
    .map(|prepared| prepared.messages)
    .unwrap_or(prepared);

    // Resolve the provider's effective context window so the harness can run the
    // context-window summarization step (issue #4249) on channel/CLI turns too —
    // long-running channel threads otherwise grew unbounded until the cap error.
    let context_window = provider.effective_context_window(model).await;

    tracing::info!(
        model,
        max_iterations,
        observed = on_progress.is_some(),
        context_window,
        "[channel:graph] routing channel turn through tinyagents harness"
    );
    let outcome = run_turn_via_tinyagents_shared(
        provider,
        model,
        temperature,
        prepared,
        vec![extra_arc, tools_registry],
        allowed,
        max_iterations,
        // Mirror the harness event stream onto AgentProgress when the caller
        // (e.g. channel dispatch) supplied a progress sink.
        on_progress,
        // Top-level (parent) turn — no child-progress attribution.
        None,
        // Resolved above — drives the context-window summarization step.
        context_window,
        // No mid-flight steering on the channel path.
        None,
        // No early-exit pause on the channel path.
        &[],
        // Channels surface the cap as an error (legacy `ErrorCheckpoint`), so no
        // graceful cap pause/summary here.
        false,
        // Bound the model's per-call output (legacy parity — channel turns ran at
        // the standard per-turn budget).
        Some(crate::openhuman::inference::provider::AGENT_TURN_MAX_OUTPUT_TOKENS),
    )
    .await?;
    // Persist the original (un-expanded) prior turns plus only the new turns the
    // run produced. `outcome.history` is `prepared` (expanded) followed by the
    // turn's new messages; `prepared` maps 1:1 onto the input, so skipping
    // `prior_len` drops the expanded copies and keeps the durable markers intact.
    history.extend(outcome.history.into_iter().skip(prior_len));
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
