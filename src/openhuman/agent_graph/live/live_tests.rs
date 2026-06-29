//! Phase A proof: the `agent_graph` engine drives a real provider + tool loop.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use super::{run_turn_via_graph, LiveToolExecutor, LiveTurnMachine};
use crate::openhuman::inference::provider::{ChatMessage, ChatResponse, Provider, ToolCall};

/// Scripted provider: first call asks for a tool, second returns a final answer.
struct ScriptedProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        Ok("unused".to_string())
    }

    async fn chat(
        &self,
        _request: crate::openhuman::inference::provider::ChatRequest<'_>,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            // First turn: request a tool call.
            Ok(ChatResponse {
                text: Some("Let me look that up.".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    arguments: r#"{"q":"weather"}"#.to_string(),
                    extra_content: None,
                }],
                ..Default::default()
            })
        } else {
            // Second turn: final answer, no tool calls.
            Ok(ChatResponse {
                text: Some("It is sunny.".to_string()),
                ..Default::default()
            })
        }
    }

    fn supports_native_tools(&self) -> bool {
        true
    }
}

/// Records executed tool calls and returns a canned result.
struct RecordingExecutor {
    executed: AtomicUsize,
}

#[async_trait]
impl LiveToolExecutor for RecordingExecutor {
    async fn execute(&self, name: &str, _arguments: &str) -> (String, bool) {
        self.executed.fetch_add(1, Ordering::SeqCst);
        (format!("{name} result: 72F"), true)
    }
}

#[tokio::test]
async fn engine_drives_a_real_provider_and_tool_loop() {
    let provider = Arc::new(ScriptedProvider {
        calls: AtomicUsize::new(0),
    });
    let executor = Arc::new(RecordingExecutor {
        executed: AtomicUsize::new(0),
    });
    let machine = LiveTurnMachine::new(
        provider.clone(),
        "mock-model",
        0.2,
        vec![ChatMessage::user("what's the weather?")],
        vec![], // no specs needed for the mock
        executor.clone(),
        10,
    );

    let outcome = run_turn_via_graph(machine).await.expect("live turn runs");

    // Two provider calls: tool-call turn, then final answer.
    assert_eq!(outcome.iterations, 2);
    assert_eq!(outcome.text, "It is sunny.");
    assert!(!outcome.hit_cap);
    // The tool was actually executed by the graph's Tools node.
    assert_eq!(executor.executed.load(Ordering::SeqCst), 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn live_turn_respects_iteration_cap() {
    // Provider that always asks for a tool → would loop forever without a cap.
    struct AlwaysTool;
    #[async_trait]
    impl Provider for AlwaysTool {
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
            Ok(ChatResponse {
                text: Some("still working".to_string()),
                tool_calls: vec![ToolCall {
                    id: "c".to_string(),
                    name: "noop".to_string(),
                    arguments: "{}".to_string(),
                    extra_content: None,
                }],
                ..Default::default()
            })
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }
    let executor = Arc::new(RecordingExecutor {
        executed: AtomicUsize::new(0),
    });
    let machine = LiveTurnMachine::new(
        Arc::new(AlwaysTool),
        "mock-model",
        0.0,
        vec![ChatMessage::user("go")],
        vec![],
        executor,
        3,
    );
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    assert!(outcome.hit_cap);
    assert_eq!(outcome.iterations, 3);
}

/// Phase B: a real harness `Tool` runs through the graph-driven turn via
/// `HarnessToolExecutor` — proving the live path executes the same tools the
/// legacy loop does, not just a mock closure.
#[tokio::test]
async fn harness_tool_executor_runs_a_real_tool_through_the_graph() {
    use super::HarnessToolExecutor;
    use crate::openhuman::tools::{Tool, ToolResult};

    // A minimal real Tool: echoes its `msg` argument back.
    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echo the msg argument."
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "msg": { "type": "string" } },
                "required": ["msg"]
            })
        }
        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
            let msg = args.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolResult::success(format!("echo: {msg}")))
        }
    }

    // Provider: first turn calls echo, second turn produces the final answer.
    struct EchoProvider {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Provider for EchoProvider {
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
                        id: "e1".to_string(),
                        name: "echo".to_string(),
                        arguments: r#"{"msg":"hi"}"#.to_string(),
                        extra_content: None,
                    }],
                    ..Default::default()
                })
            } else {
                Ok(ChatResponse {
                    text: Some("done".to_string()),
                    ..Default::default()
                })
            }
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }

    let executor = Arc::new(HarnessToolExecutor::new(vec![Arc::new(EchoTool)]));
    // The executor advertises the real tool spec.
    assert_eq!(executor.specs().len(), 1);
    let machine = LiveTurnMachine::new(
        Arc::new(EchoProvider {
            calls: AtomicUsize::new(0),
        }),
        "mock-model",
        0.0,
        vec![ChatMessage::user("echo hi please")],
        executor.specs(),
        executor.clone(),
        10,
    );
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    assert_eq!(outcome.iterations, 2);
    assert_eq!(outcome.text, "done");
}
