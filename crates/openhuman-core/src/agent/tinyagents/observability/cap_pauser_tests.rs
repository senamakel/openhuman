use super::*;

use std::time::Duration;

use async_trait::async_trait;
use tinyagents_harness::context::{RunConfig, RunContext};
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::events::EventSink;
use tinyagents_harness::middleware::{AgentRun, Middleware};
use tinyagents_harness::runtime::{AgentHarness, RunPolicy};
use tinyagents_harness::steering::SteeringPolicy;
use tinyagents_harness::subagent::SubAgent;
use tinyagents_harness::testkit::{FakeTool, ScriptedModel, SlowModel};
use tinyagents_harness::tool::ToolResult;
use tinyinference::message::{AssistantMessage, Message};
use tinyinference::model::{ChatModel, ModelResponse};
use tinyinference::tool::ToolCall;

const CAP: usize = 3;

fn tool_call_response(id: &str) -> ModelResponse {
    ModelResponse {
        message: AssistantMessage {
            id: Some(format!("msg-{id}")),
            content: Vec::new(),
            tool_calls: vec![ToolCall::new(id, "lookup", serde_json::json!({}))],
            usage: None,
        },
        usage: None,
        finish_reason: Some("tool_calls".to_string()),
        raw: None,
        resolved_model: None,
        continue_turn: None,
        served_from_cache: false,
    }
}

fn child_harness(model: Arc<dyn ChatModel<()>>) -> Arc<AgentHarness<()>> {
    let mut child: AgentHarness<()> = AgentHarness::new();
    child
        .register_model("child", model)
        .set_default_model("child");
    Arc::new(child)
}

/// Drive a parent run capped at [`CAP`] model calls whose every tool result
/// passes through `middleware`, with a [`CapPauser`] on the shared sink.
async fn run_capped_parent(middleware: Arc<dyn Middleware<()>>) -> AgentRun {
    let mut harness: AgentHarness<()> = AgentHarness::new();
    let mut policy = RunPolicy::default();
    // Hard ceiling well above the cap, so only the pauser can stop the run.
    policy.limits.max_model_calls = 10 * CAP;
    policy.limits.max_tool_calls = 80 * CAP;
    harness.with_policy(policy);
    let replies = (1..=10 * CAP)
        .map(|i| tool_call_response(&format!("t{i}")))
        .collect();
    harness
        .register_model("parent", Arc::new(ScriptedModel::new(replies)))
        .set_default_model("parent");
    harness.register_tool(Arc::new(FakeTool::returning("lookup", "ok")));
    harness.push_middleware(middleware);

    let sink = EventSink::new();
    let handle = SteeringHandle::new(SteeringPolicy::allow_all());
    let ctx = RunContext::new(RunConfig::new("agent_turn"), ())
        .with_events(sink.clone())
        .with_steering(handle.clone());
    sink.subscribe(CapPauser::new(handle, CAP, ctx.run_id().as_str(), None));

    harness
        .invoke_in_context(&(), ctx, vec![Message::user("go")])
        .await
        .expect("run completes")
}

/// Runs a one-call nested sub-agent on the parent's sink after every tool, the
/// way the payload summarizer does for an oversized result. The child is named
/// after the parent run so its run id (`agent_turn-d1-N`) shares the parent's
/// prefix — attribution must still tell the two apart.
struct NestedRunAfterEveryTool;

#[async_trait]
impl Middleware<()> for NestedRunAfterEveryTool {
    fn name(&self) -> &str {
        "nested_run_after_every_tool"
    }

    async fn after_tool(
        &self,
        ctx: &mut RunContext<()>,
        _state: &(),
        _result: &mut ToolResult,
    ) -> TaResult<()> {
        let model: Arc<dyn ChatModel<()>> = Arc::new(ScriptedModel::replies(vec!["summary"]));
        SubAgent::new("agent_turn", "nested", child_harness(model))
            .invoke_with_events(&(), (), ctx.depth(), "summarize", &ctx.events)
            .await?;
        Ok(())
    }
}

/// Fans out two sibling sub-agents on the parent's sink after every tool and
/// joins them. Each child's model sleeps before answering, so both runs are in
/// flight at once and their events interleave on the shared sink.
struct OverlappingNestedRunsAfterEveryTool;

#[async_trait]
impl Middleware<()> for OverlappingNestedRunsAfterEveryTool {
    fn name(&self) -> &str {
        "overlapping_nested_runs_after_every_tool"
    }

    async fn after_tool(
        &self,
        ctx: &mut RunContext<()>,
        _state: &(),
        _result: &mut ToolResult,
    ) -> TaResult<()> {
        let depth = ctx.depth();
        let events = ctx.events.clone();
        let slow = || -> Arc<dyn ChatModel<()>> {
            Arc::new(SlowModel::new(Duration::from_millis(20), "summary"))
        };
        let first = SubAgent::new("agent_turn", "nested", child_harness(slow()));
        let second = SubAgent::new("summarizer", "nested", child_harness(slow()));
        let (first, second) = futures::join!(
            first.invoke_with_events(&(), (), depth, "first", &events),
            second.invoke_with_events(&(), (), depth, "second", &events),
        );
        first?;
        second?;
        Ok(())
    }
}

#[tokio::test]
async fn nested_runs_on_the_shared_sink_do_not_spend_the_parents_cap() {
    let run = run_capped_parent(Arc::new(NestedRunAfterEveryTool)).await;

    assert!(run.paused.is_some(), "the cap pauser must pause the run");
    assert_eq!(
        run.model_calls, CAP,
        "the pause must land after the run's OWN {CAP} model calls; a nested run's \
         completions on the shared sink must not count toward the parent's cap"
    );
}

#[tokio::test]
async fn overlapping_sibling_runs_on_the_shared_sink_do_not_spend_the_parents_cap() {
    let run = run_capped_parent(Arc::new(OverlappingNestedRunsAfterEveryTool)).await;

    assert!(run.paused.is_some(), "the cap pauser must pause the run");
    assert_eq!(
        run.model_calls, CAP,
        "concurrent sibling sub-agents on the shared sink must not count toward the \
         parent's {CAP}-call cap"
    );
}

/// The agent loop awaits tool middleware, so inside one harness run a parent
/// completion can only land between child completions when a child outlives the
/// tool call that started it. Drive that interleaving directly on the listener,
/// with the crate's id formats, so the order is exact rather than timing-bound.
#[test]
fn parent_completions_interleaved_with_active_children_pause_only_on_the_runs_own_cap() {
    let sink = EventSink::new();
    let handle = SteeringHandle::new(SteeringPolicy::allow_all());
    sink.subscribe(CapPauser::new(handle.clone(), CAP, "agent_turn", None));
    let complete = |call_id: &str| {
        sink.emit(AgentEvent::ModelCompleted {
            call_id: call_id.into(),
            started_at_ms: None,
            usage: None,
            input: None,
            output: None,
        });
    };

    for call_id in [
        "agent_turn-model-1",
        "agent_turn-d1-7-model-1",
        "summarizer-d1-8-model-1",
        "agent_turn-model-2",
        "agent_turn-d1-7-model-2",
        "summarizer-d2-9-model-1",
        "agent_turn-d1-7-model-3",
    ] {
        complete(call_id);
    }
    assert_eq!(
        handle.pending(),
        0,
        "five nested completions interleaved with two of the run's own must not request a pause"
    );

    complete("agent_turn-model-3");
    assert_eq!(
        handle.pending(),
        1,
        "the pause must be requested exactly when the run's own {CAP}rd model call completes"
    );
}
