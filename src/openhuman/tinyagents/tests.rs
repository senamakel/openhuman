//! End-to-end tests for the `tinyagents` harness route: a real openhuman
//! [`Provider`] and [`Tool`] driven through [`run_turn_via_tinyagents`].

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use super::*;
use crate::openhuman::inference::provider::{ChatRequest, ChatResponse, Provider, ToolCall};
use crate::openhuman::tools::{Tool, ToolResult};

/// A real openhuman tool the harness will execute.
struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "echoes its msg argument"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "msg": { "type": "string" } },
            "required": ["msg"]
        })
    }
    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let m = args.get("msg").and_then(|v| v.as_str()).unwrap_or("");
        Ok(ToolResult::success(format!("echoed:{m}")))
    }
}

/// Mock provider: first call requests the echo tool, second call answers.
struct EchoThenDone {
    calls: AtomicUsize,
}

#[async_trait]
impl Provider for EchoThenDone {
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
        _r: ChatRequest<'_>,
        _model: &str,
        _t: f64,
    ) -> anyhow::Result<ChatResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Ok(ChatResponse {
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
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
async fn turn_runs_through_the_tinyagents_harness_with_real_tools() {
    let provider = Arc::new(EchoThenDone {
        calls: AtomicUsize::new(0),
    });
    let history = vec![ChatMessage::user("please echo hi")];
    let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(EchoTool)];

    let outcome = run_turn_via_tinyagents(provider, "mock-model", 0.0, history, tools, 10)
        .await
        .expect("tinyagents harness turn runs");

    assert_eq!(outcome.text, "all done");
    assert!(outcome.model_calls >= 2, "expected >=2 model calls");
    assert!(outcome.tool_calls >= 1, "expected the echo tool to run");
    assert!(
        outcome
            .history
            .iter()
            .any(|m| m.content.contains("echoed:hi")),
        "tool result should be threaded into the transcript: {:?}",
        outcome.history
    );
}

#[test]
fn routing_flag_defaults_off() {
    std::env::remove_var("OPENHUMAN_AGENT_GRAPH_TINYAGENTS");
    assert!(!tinyagents_routing_enabled());
}
