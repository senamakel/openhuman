//! Phase C (issue #4249): optionally run a channel/CLI agent turn through the
//! `agent_graph` engine instead of the legacy `run_tool_call_loop`.
//!
//! Gated by `OPENHUMAN_AGENT_GRAPH_CHANNEL` and **off by default**. Like the
//! sub-agent route, this reuses the same provider + tools via
//! [`SharedToolExecutor`] over the bus handler's `Arc`-shared tool sets
//! (`tools_registry: Arc<Vec<Box<dyn Tool>>>` + per-turn extras), so no engine
//! lifetime change is needed. The live path already covers the loop's
//! control-flow seams (iteration cap, circuit breakers, stop hooks,
//! token-budget trimming, multimodal prep). Token-streaming deltas /
//! `AgentProgress` mirroring are not yet forwarded on this path, so it stays
//! opt-in until that is migrated.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use crate::openhuman::agent_graph::live::{
    run_turn_via_graph, LiveTurnMachine, SharedToolExecutor,
};
use crate::openhuman::config::{MultimodalConfig, MultimodalFileConfig};
use crate::openhuman::inference::provider::{ChatMessage, Provider};
use crate::openhuman::tools::Tool;

/// Whether channel/CLI turns should route through the `agent_graph` engine.
pub(crate) fn channel_graph_routing_enabled() -> bool {
    std::env::var("OPENHUMAN_AGENT_GRAPH_CHANNEL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Drive a channel/CLI turn on the graph engine. Returns the final assistant
/// text, matching [`super::tool_loop::run_tool_call_loop`].
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
) -> Result<String> {
    let extra_arc = Arc::new(extra_tools);

    // Advertised specs: registry + per-turn extras, filtered by the visibility
    // whitelist, deduped by name (newest wins — extras shadow registry).
    let mut seen = HashSet::new();
    let mut specs = Vec::new();
    for tool in extra_arc.iter().chain(tools_registry.iter()) {
        let name = tool.name().to_string();
        let visible = visible_tool_names
            .map(|w| w.contains(&name))
            .unwrap_or(true);
        if visible && seen.insert(name) {
            specs.push(tool.spec());
        }
    }

    let allowed = visible_tool_names.cloned().unwrap_or_default();
    let executor = Arc::new(SharedToolExecutor::new(
        vec![extra_arc, tools_registry],
        allowed,
    ));

    tracing::info!(
        model,
        max_iterations,
        tool_specs = specs.len(),
        "[channel:graph] routing channel turn through agent_graph engine"
    );
    let machine = LiveTurnMachine::new(
        provider,
        model,
        temperature,
        history.clone(),
        specs,
        executor,
        max_iterations,
    )
    .with_multimodal(multimodal, multimodal_files);
    let outcome = run_turn_via_graph(machine).await?;
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
        )
        .await
        .expect("channel graph turn runs");
        assert_eq!(text, "channel done");
        assert!(history.iter().any(|m| m.content.contains("pong")));
    }

    #[test]
    fn routing_flag_defaults_off() {
        std::env::remove_var("OPENHUMAN_AGENT_GRAPH_CHANNEL");
        assert!(!channel_graph_routing_enabled());
    }
}
