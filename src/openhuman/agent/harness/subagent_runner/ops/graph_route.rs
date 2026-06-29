//! Phase B (issue #4249): optionally run a sub-agent turn through the
//! `agent_graph` engine instead of the legacy `run_turn_engine`.
//!
//! Gated by `OPENHUMAN_AGENT_GRAPH_SUBAGENT` and **off by default**. The graph
//! path reuses the same provider and the same harness tools (via
//! [`SharedToolExecutor`] over the sub-agent's shared `Arc<Vec<Box<dyn Tool>>>`
//! tool sets, so no engine lifetime change is needed). It is an explicit subset
//! of the legacy loop today — it does not yet wire the payload summarizer,
//! mid-flight steering, transcript observer, or the `ask_user_clarification`
//! pause — so it is opt-in until those are migrated in the following phases.

use std::collections::HashSet;
use std::sync::Arc;

use super::loop_::AggregatedUsage;
use crate::openhuman::agent::harness::subagent_runner::types::SubagentRunError;
use crate::openhuman::agent_graph::live::{
    run_turn_via_graph, LiveTurnMachine, SharedToolExecutor,
};
use crate::openhuman::inference::provider::{ChatMessage, Provider};
use crate::openhuman::tools::{Tool, ToolSpec};

/// Whether sub-agent turns should be routed through the `agent_graph` engine.
pub(super) fn subagent_graph_routing_enabled() -> bool {
    std::env::var("OPENHUMAN_AGENT_GRAPH_SUBAGENT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Drive a sub-agent turn on the graph engine. Returns the same tuple shape as
/// [`super::loop_::run_inner_loop`] so the caller is agnostic to the path.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_subagent_via_graph(
    provider: Arc<dyn Provider>,
    model: &str,
    temperature: f64,
    history: &mut Vec<ChatMessage>,
    parent_tools: Arc<Vec<Box<dyn Tool>>>,
    dynamic_tools: Vec<Box<dyn Tool>>,
    specs: Vec<ToolSpec>,
    allowed_names: HashSet<String>,
    max_iterations: usize,
    run_queue: Option<Arc<crate::openhuman::agent::harness::run_queue::RunQueue>>,
) -> Result<(String, usize, AggregatedUsage, Option<String>), SubagentRunError> {
    tracing::info!(
        model,
        max_iterations,
        "[subagent_runner:graph] routing sub-agent turn through agent_graph engine"
    );
    let executor = Arc::new(SharedToolExecutor::new(
        vec![parent_tools, Arc::new(dynamic_tools)],
        allowed_names,
    ));
    let machine = LiveTurnMachine::new(
        provider,
        model,
        temperature,
        history.clone(),
        specs,
        executor,
        max_iterations,
    )
    // `ask_user_clarification` pauses the sub-agent so the orchestrator can
    // relay the user's answer (parity with the legacy early-exit path).
    .with_early_exit_tools(vec!["ask_user_clarification".to_string()])
    .with_run_queue(run_queue);
    let outcome = run_turn_via_graph(machine).await?;
    // Write the final conversation back so the caller can checkpoint / persist.
    *history = outcome.history;
    let usage = AggregatedUsage {
        input_tokens: outcome.cost.input_tokens,
        output_tokens: outcome.cost.output_tokens,
        cached_input_tokens: outcome.cost.cached_input_tokens,
        charged_amount_usd: outcome.cost.total_usd(),
    };
    Ok((
        outcome.text,
        outcome.iterations as usize,
        usage,
        outcome.early_exit_tool,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::inference::provider::{ChatResponse, ToolCall};
    use crate::openhuman::tools::ToolResult;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
            let m = args.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolResult::success(format!("echoed:{m}")))
        }
    }

    struct TwoStepProvider {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Provider for TwoStepProvider {
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
                        id: "1".to_string(),
                        name: "echo".to_string(),
                        arguments: r#"{"msg":"hi"}"#.to_string(),
                        extra_content: None,
                    }],
                    ..Default::default()
                })
            } else {
                Ok(ChatResponse {
                    text: Some("all done".to_string()),
                    ..Default::default()
                })
            }
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn subagent_runs_through_the_graph_engine_with_real_tools() {
        let provider = Arc::new(TwoStepProvider {
            calls: AtomicUsize::new(0),
        });
        let parent_tools: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![Box::new(EchoTool)]);
        let mut allowed = HashSet::new();
        allowed.insert("echo".to_string());
        let mut history = vec![ChatMessage::user("please echo hi")];

        let (output, iterations, usage, early_exit) = run_subagent_via_graph(
            provider,
            "mock-model",
            0.0,
            &mut history,
            parent_tools,
            vec![],
            vec![],
            allowed,
            10,
            None,
        )
        .await
        .expect("graph subagent runs");

        assert_eq!(output, "all done");
        assert_eq!(iterations, 2);
        assert!(early_exit.is_none());
        let _ = usage;
        // History was written back: user + assistant(tool) + tool result + assistant(final).
        assert!(history.len() >= 4);
        assert!(history.iter().any(|m| m.content.contains("echoed:hi")));
    }

    #[test]
    fn routing_flag_defaults_off() {
        // No env var set in the default test environment.
        std::env::remove_var("OPENHUMAN_AGENT_GRAPH_SUBAGENT");
        assert!(!subagent_graph_routing_enabled());
    }
}
