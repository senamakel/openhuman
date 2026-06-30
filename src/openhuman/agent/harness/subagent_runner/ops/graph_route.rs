//! Phase B (issue #4249): optionally run a sub-agent turn through the
//! `agent_graph` engine instead of the legacy `run_turn_engine`.
//!
//! **Default ON in production** (issue #4249); set `OPENHUMAN_AGENT_GRAPH_SUBAGENT=0`
//! to fall back to the legacy `run_inner_loop`. The tinyagents path reuses the
//! same provider and the same harness tools (via [`SharedToolAdapter`] over the
//! sub-agent's shared `Arc<Vec<Box<dyn Tool>>>` tool sets, so no engine lifetime
//! change is needed). It now mirrors the legacy seams: child progress deltas
//! (`Subagent*` events incl. thinking), mid-flight steering, autocompaction, and
//! the `ask_user_clarification` early-exit pause. Remaining gap: the payload
//! summarizer / transcript observer details.

use std::collections::HashSet;
use std::sync::Arc;

use super::loop_::AggregatedUsage;
use crate::openhuman::agent::harness::subagent_runner::types::SubagentRunError;
use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::inference::provider::{ChatMessage, Provider};
use crate::openhuman::tinyagents::{run_turn_via_tinyagents_shared, SubagentScope};
use crate::openhuman::tools::{Tool, ToolSpec};

/// Whether sub-agent turns should be routed through the tinyagents harness.
///
/// **Default ON in production** (issue #4249); OFF under `cfg(test)` so the
/// legacy-engine tests keep validating the fallback. Set
/// `OPENHUMAN_AGENT_GRAPH_SUBAGENT=0` to force the legacy `run_inner_loop`.
pub(super) fn subagent_graph_routing_enabled() -> bool {
    crate::openhuman::tinyagents::routing_default_with_override("OPENHUMAN_AGENT_GRAPH_SUBAGENT")
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
    on_progress: Option<tokio::sync::mpsc::Sender<AgentProgress>>,
    agent_id: &str,
    task_id: &str,
    extended_policy: bool,
) -> Result<(String, usize, AggregatedUsage, Option<String>), SubagentRunError> {
    tracing::info!(
        model,
        max_iterations,
        agent_id,
        task_id,
        observed = on_progress.is_some(),
        "[subagent_runner:graph] routing sub-agent turn through tinyagents harness"
    );
    // `specs` is derived from the registry inside the runner; the tinyagents
    // adapters advertise each tool via its own `spec()`, so it's unused here.
    let _ = &specs;

    // Child-progress attribution: when the parent carries an `on_progress` sink,
    // mirror this sub-agent's iterations / tool calls / text + thinking deltas as
    // `Subagent*` events scoped to (`agent_id`, `task_id`) so the parent thread
    // can nest them under the live subagent row.
    let subagent_scope = on_progress.as_ref().map(|_| SubagentScope {
        agent_id: agent_id.to_string(),
        task_id: task_id.to_string(),
        extended_policy,
    });

    let outcome = run_turn_via_tinyagents_shared(
        provider,
        model,
        temperature,
        history.clone(),
        vec![parent_tools, Arc::new(dynamic_tools)],
        allowed_names,
        max_iterations,
        // Parent's progress sink — child events ride it, scoped below.
        on_progress,
        subagent_scope,
        // Sub-agents inherit the parent's already-trimmed history.
        None,
        // Mid-flight steering: forward queued steer messages into the run.
        run_queue,
        // Pause + checkpoint when the child asks the user a clarifying question.
        &["ask_user_clarification"],
    )
    .await
    .map_err(SubagentRunError::Provider)?;

    // Write the final conversation back so the caller can checkpoint / persist.
    *history = outcome.history;
    let usage = AggregatedUsage {
        input_tokens: outcome.input_tokens,
        output_tokens: outcome.output_tokens,
        cached_input_tokens: 0,
        charged_amount_usd: 0.0,
    };
    // On an early-exit (`ask_user_clarification`), `outcome.text` is the question
    // and the runner checkpoints + returns AwaitingUser. `None` = ran to a final
    // answer.
    Ok((
        outcome.text,
        outcome.model_calls,
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
            None,
            "researcher",
            "task-1",
            false,
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

    /// A provider that streams visible text + reasoning through the request's
    /// delta sender, exercising the child-progress bridge end to end.
    struct ThinkingStreamProvider;
    #[async_trait]
    impl Provider for ThinkingStreamProvider {
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
            r: crate::openhuman::inference::provider::ChatRequest<'_>,
            _model: &str,
            _t: f64,
        ) -> anyhow::Result<ChatResponse> {
            use crate::openhuman::inference::provider::ProviderDelta;
            if let Some(tx) = r.stream {
                let _ = tx
                    .send(ProviderDelta::ThinkingDelta {
                        delta: "let me think".into(),
                    })
                    .await;
                for chunk in ["Hel", "lo"] {
                    let _ = tx
                        .send(ProviderDelta::TextDelta {
                            delta: chunk.into(),
                        })
                        .await;
                }
            }
            Ok(ChatResponse {
                text: Some("Hello".to_string()),
                ..Default::default()
            })
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn child_text_and_thinking_deltas_are_scoped_to_the_subagent() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentProgress>(64);
        let parent_tools: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![]);
        let mut history = vec![ChatMessage::user("hi")];

        let (output, _iters, _usage, _early) = run_subagent_via_graph(
            Arc::new(ThinkingStreamProvider),
            "mock-model",
            0.0,
            &mut history,
            parent_tools,
            vec![],
            vec![],
            HashSet::new(),
            4,
            None,
            Some(tx),
            "researcher",
            "task-7",
            false,
        )
        .await
        .expect("child-delta subagent runs");

        assert_eq!(output, "Hello");

        let mut text = String::new();
        let mut thinking = String::new();
        let mut saw_iter = false;
        while let Ok(p) = rx.try_recv() {
            match p {
                AgentProgress::SubagentTextDelta { delta, task_id, .. } => {
                    assert_eq!(task_id, "task-7");
                    text.push_str(&delta);
                }
                AgentProgress::SubagentThinkingDelta {
                    delta, agent_id, ..
                } => {
                    assert_eq!(agent_id, "researcher");
                    thinking.push_str(&delta);
                }
                AgentProgress::SubagentIterationStarted { task_id, .. } => {
                    assert_eq!(task_id, "task-7");
                    saw_iter = true;
                }
                // The parent-scoped variants must never appear on a child run.
                AgentProgress::TextDelta { .. }
                | AgentProgress::ThinkingDelta { .. }
                | AgentProgress::IterationStarted { .. } => {
                    panic!("child run emitted a parent-scoped progress event");
                }
                _ => {}
            }
        }
        assert!(saw_iter, "a SubagentIterationStarted should be emitted");
        assert!(
            text.contains("Hello"),
            "child text deltas should reassemble, got {text:?}"
        );
        assert!(
            thinking.contains("let me think"),
            "child thinking deltas should be forwarded, got {thinking:?}"
        );
    }

    /// A tool named like the early-exit tool that echoes its `question` arg.
    struct AskTool;
    #[async_trait]
    impl Tool for AskTool {
        fn name(&self) -> &str {
            "ask_user_clarification"
        }
        fn description(&self) -> &str {
            "ask the user a clarifying question"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {"question": {"type": "string"}}})
        }
        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
            let q = args
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Ok(ToolResult::success(q))
        }
    }

    /// A provider whose first turn calls `ask_user_clarification`; a second turn
    /// would answer, but the early-exit pause should stop the loop before it.
    struct AskThenAnswer {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Provider for AskThenAnswer {
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
                        id: "ask-1".to_string(),
                        name: "ask_user_clarification".to_string(),
                        arguments: r#"{"question":"which file?"}"#.to_string(),
                        extra_content: None,
                    }],
                    ..Default::default()
                })
            } else {
                Ok(ChatResponse {
                    text: Some("should not be reached".to_string()),
                    ..Default::default()
                })
            }
        }
        fn supports_native_tools(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn ask_user_clarification_pauses_and_surfaces_the_question() {
        let provider = Arc::new(AskThenAnswer {
            calls: AtomicUsize::new(0),
        });
        let parent_tools: Arc<Vec<Box<dyn Tool>>> = Arc::new(vec![Box::new(AskTool)]);
        let mut allowed = HashSet::new();
        allowed.insert("ask_user_clarification".to_string());
        let mut history = vec![ChatMessage::user("help me")];

        let (output, iterations, _usage, early_exit) = run_subagent_via_graph(
            provider.clone(),
            "mock-model",
            0.0,
            &mut history,
            parent_tools,
            vec![],
            vec![],
            allowed,
            10,
            None,
            None,
            "researcher",
            "task-9",
            false,
        )
        .await
        .expect("ask-clarification subagent runs");

        // The loop paused after the tool round: the early-exit tool is surfaced
        // and the question is the returned text — the second model turn never ran.
        assert_eq!(early_exit.as_deref(), Some("ask_user_clarification"));
        assert_eq!(output, "which file?");
        assert_eq!(
            iterations, 1,
            "the loop should pause before a second model call"
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn routing_flag_honors_explicit_override() {
        // Bare default OFF under cfg(test); production defaults ON.
        std::env::remove_var("OPENHUMAN_AGENT_GRAPH_SUBAGENT");
        assert!(!subagent_graph_routing_enabled());
        std::env::set_var("OPENHUMAN_AGENT_GRAPH_SUBAGENT", "1");
        assert!(subagent_graph_routing_enabled());
        std::env::remove_var("OPENHUMAN_AGENT_GRAPH_SUBAGENT");
    }
}
