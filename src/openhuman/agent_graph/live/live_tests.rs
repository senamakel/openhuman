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

/// Phase C parity: an early-exit tool (e.g. ask_user_clarification) stops the
/// loop and is surfaced so the caller can pause.
#[tokio::test]
async fn early_exit_tool_pauses_the_live_turn() {
    use super::HarnessToolExecutor;
    use crate::openhuman::tools::{Tool, ToolResult};

    struct AskTool;
    #[async_trait]
    impl Tool for AskTool {
        fn name(&self) -> &str {
            "ask_user_clarification"
        }
        fn description(&self) -> &str {
            "ask"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success("Which file did you mean?"))
        }
    }
    struct AskProvider;
    #[async_trait]
    impl Provider for AskProvider {
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
                tool_calls: vec![ToolCall {
                    id: "a".to_string(),
                    name: "ask_user_clarification".to_string(),
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
    let executor = Arc::new(HarnessToolExecutor::new(vec![Arc::new(AskTool)]));
    let machine = LiveTurnMachine::new(
        Arc::new(AskProvider),
        "mock-model",
        0.0,
        vec![ChatMessage::user("do the thing")],
        executor.specs(),
        executor,
        10,
    )
    .with_early_exit_tools(vec!["ask_user_clarification".to_string()]);
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    assert_eq!(
        outcome.early_exit_tool.as_deref(),
        Some("ask_user_clarification")
    );
    assert_eq!(outcome.text, "Which file did you mean?");
    // Paused on the first iteration.
    assert_eq!(outcome.iterations, 1);
}

/// Phase C parity: the repeated-failure circuit breaker halts a stuck loop
/// instead of grinding to the iteration cap.
#[tokio::test]
async fn circuit_breaker_halts_repeated_tool_failure() {
    use super::HarnessToolExecutor;
    use crate::openhuman::tools::{Tool, ToolResult};

    struct FailTool;
    #[async_trait]
    impl Tool for FailTool {
        fn name(&self) -> &str {
            "broken"
        }
        fn description(&self) -> &str {
            "always fails"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::error("permanent failure"))
        }
    }
    struct RetryProvider;
    #[async_trait]
    impl Provider for RetryProvider {
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
                text: Some("retrying".to_string()),
                tool_calls: vec![ToolCall {
                    id: "x".to_string(),
                    name: "broken".to_string(),
                    arguments: r#"{"n":1}"#.to_string(),
                    extra_content: None,
                }],
                ..Default::default()
            })
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }
    let executor = Arc::new(HarnessToolExecutor::new(vec![Arc::new(FailTool)]));
    let machine = LiveTurnMachine::new(
        Arc::new(RetryProvider),
        "mock-model",
        0.0,
        vec![ChatMessage::user("use broken")],
        executor.specs(),
        executor,
        100, // high cap — the breaker, not the cap, must stop it
    );
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    // Halted well before the 100-iteration cap.
    assert!(
        outcome.iterations < 100,
        "breaker should halt early, got {}",
        outcome.iterations
    );
    assert!(!outcome.hit_cap);
}

/// Phase C parity: an installed stop hook (e.g. max-iterations policy) halts the
/// live turn — the same `with_stop_hooks` surface the legacy loop honors.
#[tokio::test]
async fn stop_hook_halts_the_live_turn() {
    use crate::openhuman::agent::stop_hooks::{with_stop_hooks, MaxIterationsStopHook};

    // Provider that always asks for a tool (would run to the cap of 50).
    struct LoopProvider;
    #[async_trait]
    impl Provider for LoopProvider {
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
                text: Some("looping".to_string()),
                tool_calls: vec![ToolCall {
                    id: "t".to_string(),
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
        Arc::new(LoopProvider),
        "mock-model",
        0.0,
        vec![ChatMessage::user("go")],
        vec![],
        executor,
        50,
    );
    // Install a 2-iteration stop hook for the duration of the run.
    let hooks: Vec<std::sync::Arc<dyn crate::openhuman::agent::stop_hooks::StopHook>> =
        vec![std::sync::Arc::new(MaxIterationsStopHook::new(2))];
    let outcome = with_stop_hooks(hooks, run_turn_via_graph(machine))
        .await
        .expect("runs");
    // Halted by the hook well before the cap of 50.
    assert!(
        outcome.iterations <= 3,
        "stop hook should halt early, got {}",
        outcome.iterations
    );
    assert!(!outcome.hit_cap);
    assert!(outcome.text.contains("stopped by hook"));
}

/// Phase C parity: an oversized tool result is routed through the payload
/// summarizer before entering history.
#[tokio::test]
async fn payload_summarizer_compresses_tool_output() {
    use super::HarnessToolExecutor;
    use crate::openhuman::agent::harness::payload_summarizer::{
        PayloadSummarizer, SummarizedPayload,
    };
    use crate::openhuman::tools::{Tool, ToolResult};

    struct BigTool;
    #[async_trait]
    impl Tool for BigTool {
        fn name(&self) -> &str {
            "big"
        }
        fn description(&self) -> &str {
            "returns a lot"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(&self, _a: serde_json::Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult::success("X".repeat(10_000)))
        }
    }
    struct Squeeze;
    #[async_trait]
    impl PayloadSummarizer for Squeeze {
        async fn maybe_summarize(
            &self,
            _tool: &str,
            _hint: Option<&str>,
            raw: &str,
        ) -> anyhow::Result<Option<SummarizedPayload>> {
            Ok(Some(SummarizedPayload {
                summary: "SUMMARY".to_string(),
                original_bytes: raw.len(),
                summary_bytes: 7,
            }))
        }
    }
    struct OneToolThenDone {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Provider for OneToolThenDone {
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
                        id: "b".to_string(),
                        name: "big".to_string(),
                        arguments: "{}".to_string(),
                        extra_content: None,
                    }],
                    ..Default::default()
                })
            } else {
                Ok(ChatResponse {
                    text: Some("ok".to_string()),
                    ..Default::default()
                })
            }
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }
    let executor = Arc::new(HarnessToolExecutor::new(vec![Arc::new(BigTool)]));
    let machine = LiveTurnMachine::new(
        Arc::new(OneToolThenDone {
            calls: AtomicUsize::new(0),
        }),
        "mock-model",
        0.0,
        vec![ChatMessage::user("get big")],
        executor.specs(),
        executor,
        10,
    )
    .with_payload_summarizer(Arc::new(Squeeze), None);
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    // The 10k payload was replaced with the summary in history.
    assert!(outcome
        .history
        .iter()
        .any(|m| m.content.contains("SUMMARY")));
    assert!(!outcome
        .history
        .iter()
        .any(|m| m.content.contains(&"X".repeat(10_000))));
}

/// Phase C parity: a steering message queued mid-flight is drained into history
/// at the iteration boundary.
#[tokio::test]
async fn run_queue_steer_is_drained_into_history() {
    use crate::openhuman::agent::harness::run_queue::{QueueMode, QueuedMessage, RunQueue};

    let rq = RunQueue::new(); // returns Arc<RunQueue>
    rq.push(QueuedMessage {
        text: "actually, focus on X".to_string(),
        mode: QueueMode::Steer,
        client_id: String::new(),
        thread_id: "thr-1".to_string(),
        queued_at_ms: 0,
        model_override: None,
        temperature: None,
        profile_id: None,
        locale: None,
    })
    .await;

    // Provider returns a final answer immediately (one dispatch).
    struct FinalProvider;
    #[async_trait]
    impl Provider for FinalProvider {
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
                text: Some("done".to_string()),
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
        Arc::new(FinalProvider),
        "mock-model",
        0.0,
        vec![ChatMessage::user("start")],
        vec![],
        executor,
        10,
    )
    .with_run_queue(Some(rq));
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    assert!(outcome
        .history
        .iter()
        .any(|m| m.content.contains("actually, focus on X")));
}

/// Phase C parity: context autocompaction summarizes older turns once history
/// grows past the trigger.
#[tokio::test]
async fn autocompaction_summarizes_when_history_grows() {
    use super::LiveAutocompact;

    // Provider: counts calls; returns a summary string when asked to summarize
    // (a single user "summarize" message), else loops with a tool call until a
    // small cap. We assert the history shrank after compaction.
    struct CompactProvider {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Provider for CompactProvider {
        async fn chat_with_system(
            &self,
            _s: Option<&str>,
            msg: &str,
            _model: &str,
            _t: f64,
        ) -> anyhow::Result<String> {
            // The summarizer calls chat_with_system / chat_with_history.
            let _ = msg;
            Ok("[summary of earlier turns]".to_string())
        }
        async fn chat(
            &self,
            _r: crate::openhuman::inference::provider::ChatRequest<'_>,
            _model: &str,
            _t: f64,
        ) -> anyhow::Result<ChatResponse> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Ok(ChatResponse {
                    text: Some("working".to_string()),
                    tool_calls: vec![ToolCall {
                        id: format!("c{n}"),
                        name: "noop".to_string(),
                        arguments: format!("{{\"i\":{n}}}"),
                        extra_content: None,
                    }],
                    ..Default::default()
                })
            } else {
                Ok(ChatResponse {
                    text: Some("final".to_string()),
                    ..Default::default()
                })
            }
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }
    let executor = Arc::new(RecordingExecutor {
        executed: AtomicUsize::new(0),
    });
    // Seed a large history so the very first dispatch trips the trigger.
    let mut history = vec![ChatMessage::system("sys")];
    for i in 0..30 {
        history.push(ChatMessage::user(format!("turn {i}")));
        history.push(ChatMessage::assistant(format!("reply {i}")));
    }
    let before = history.len();
    let machine = LiveTurnMachine::new(
        Arc::new(CompactProvider {
            calls: AtomicUsize::new(0),
        }),
        "mock-model",
        0.0,
        history,
        vec![],
        executor,
        5,
    )
    .with_autocompact(LiveAutocompact {
        trigger_messages: 10,
        keep_recent: 4,
        temperature: 0.0,
        summarizer_model: None,
    });
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    // After compaction the final history is materially smaller than the seed.
    assert!(
        outcome.history.len() < before,
        "expected compaction to shrink history ({} -> {})",
        before,
        outcome.history.len()
    );
}

/// Phase C parity: multimodal preparation runs over the request (default config
/// leaves plain-text messages intact, proving the seam is wired).
#[tokio::test]
async fn multimodal_preparation_is_wired() {
    use crate::openhuman::config::{MultimodalConfig, MultimodalFileConfig};

    struct PlainProvider;
    #[async_trait]
    impl Provider for PlainProvider {
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
                text: Some("ok".to_string()),
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
        Arc::new(PlainProvider),
        "mock-model",
        0.0,
        vec![ChatMessage::user("hello")],
        vec![],
        executor,
        5,
    )
    .with_multimodal(MultimodalConfig::default(), MultimodalFileConfig::default());
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    assert_eq!(outcome.text, "ok");
}

/// Phase C parity: the per-iteration observer is invoked with the history so the
/// transcript can be persisted.
#[tokio::test]
async fn observer_fires_per_iteration() {
    use super::LiveTurnObserver;
    use std::sync::Mutex;

    struct CountingObserver {
        calls: Mutex<usize>,
        last_len: Mutex<usize>,
    }
    #[async_trait]
    impl LiveTurnObserver for CountingObserver {
        async fn after_iteration(&self, history: &[ChatMessage], _iteration: usize) {
            *self.calls.lock().unwrap() += 1;
            *self.last_len.lock().unwrap() = history.len();
        }
    }
    let obs = Arc::new(CountingObserver {
        calls: Mutex::new(0),
        last_len: Mutex::new(0),
    });
    // 2-iteration turn (tool then final).
    let provider = Arc::new(ScriptedProvider {
        calls: AtomicUsize::new(0),
    });
    let executor = Arc::new(RecordingExecutor {
        executed: AtomicUsize::new(0),
    });
    let machine = LiveTurnMachine::new(
        provider,
        "mock-model",
        0.0,
        vec![ChatMessage::user("go")],
        vec![],
        executor,
        10,
    )
    .with_observer(obs.clone());
    let outcome = run_turn_via_graph(machine).await.expect("runs");
    // Fired at least once (final), with the final history length.
    assert!(*obs.calls.lock().unwrap() >= 1);
    assert_eq!(*obs.last_len.lock().unwrap(), outcome.history.len());
}
