//! Built-in product graphs expressed on the **tinyagents** durable engine
//! (issue #4249) — the rebase target that retires the in-house `graph/` engine.
//!
//! Same topologies and node semantics as [`super::canonical_turn`] /
//! [`super::product_graphs`], re-expressed as `tinyagents::GraphBuilder` over
//! [`ProductState`] (whole-state overwrite reducer):
//!
//! - branch nodes return [`TaCommand`] `goto` (declared via
//!   [`GraphBuilder::mark_command_routing`]) instead of the in-house
//!   `NodeOutput::goto`;
//! - the HITL review node pauses via [`TaNodeResult::Interrupt`] and, on resume,
//!   routes on the human decision (the resume value arrives on the
//!   [`NodeContext`]);
//! - real archetypes run through the same [`Agent`] path as before.
//!
//! Pause/resume runs on a tinyagents [`Checkpointer`]: `run_with_thread` →
//! `resume(thread, Command::resume(..))`.

use std::sync::Arc;

use serde_json::Value;
use tinyagents::graph::{
    Command as TaCommand, CompiledGraph as TaCompiledGraph, GraphBuilder as TaGraphBuilder,
    Interrupt as TaInterrupt, NodeContext as TaNodeContext, NodeResult as TaNodeResult,
};

use super::state::ProductState;
use crate::openhuman::agent::Agent;
use crate::openhuman::config::Config;

/// A built tinyagents product graph over [`ProductState`].
pub type ProductGraph = TaCompiledGraph<ProductState, ProductState>;

fn max_iters(state: &ProductState) -> i64 {
    state
        .vars
        .get("max_iterations")
        .and_then(|v| v.as_i64())
        .unwrap_or(10)
}

fn iter_count(state: &ProductState) -> i64 {
    state
        .vars
        .get("__iter")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// Pull the human decision for a resumed review node: prefer the value carried
/// on the [`NodeContext`] (tinyagents resume), falling back to `resume_input`.
fn resume_decision(ctx: &TaNodeContext, state: &mut ProductState) -> Option<String> {
    ctx.resume
        .as_ref()
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| state.resume_input.take())
}

/// The canonical agent turn on the tinyagents engine: `dispatch → parse →
/// stop_check → tools → compact → loop, or finalize`.
pub fn build_canonical_turn() -> anyhow::Result<ProductGraph> {
    let g = TaGraphBuilder::<ProductState, ProductState>::overwrite()
        .with_graph_id("canonical_turn")
        // dispatch: advance the iteration counter, then fall through to parse.
        .add_node("dispatch", |mut s: ProductState, _c| async move {
            let next = iter_count(&s) + 1;
            s.vars.insert("__iter".to_string(), Value::from(next));
            Ok(TaNodeResult::Update(s))
        })
        // parse: route to finalize at the cap, else to stop_check.
        .add_node("parse", |s: ProductState, _c| async move {
            let target = if iter_count(&s) >= max_iters(&s) {
                "finalize"
            } else {
                "stop_check"
            };
            Ok(TaNodeResult::Command(TaCommand::goto([target])))
        })
        // stop_check: policy gate — finalize at the cap, else run tools.
        .add_node("stop_check", |s: ProductState, _c| async move {
            let target = if iter_count(&s) >= max_iters(&s) {
                "finalize"
            } else {
                "tools"
            };
            Ok(TaNodeResult::Command(TaCommand::goto([target])))
        })
        // tools: record a step, then fall through to compact.
        .add_node("tools", |mut s: ProductState, _c| async move {
            let n = iter_count(&s);
            s.record_step("tools", format!("executed tools (iteration {n})"));
            Ok(TaNodeResult::Update(s))
        })
        // compact: context management, then loop back to dispatch.
        .add_node("compact", |s: ProductState, _c| async move {
            Ok(TaNodeResult::Update(s))
        })
        // finalize: stamp the terminal answer.
        .add_node("finalize", |mut s: ProductState, _c| async move {
            let n = iter_count(&s);
            s.set_var("final", format!("completed after {n} iteration(s)"));
            s.record_step("finalize", format!("final answer after {n} iteration(s)"));
            Ok(TaNodeResult::Update(s))
        })
        .set_entry("dispatch")
        .add_edge("dispatch", "parse")
        .mark_command_routing("parse")
        .mark_command_routing("stop_check")
        .add_edge("tools", "compact")
        .add_edge("compact", "dispatch")
        .set_finish("finalize");
    g.compile()
        .map_err(|e| anyhow::anyhow!("canonical_turn compile: {e}"))
}

/// `plan_execute_review` — composes the `planner` and `code_executor`
/// archetypes around a human review gate.
pub fn build_plan_execute_review(config: Arc<Config>) -> anyhow::Result<ProductGraph> {
    let plan_cfg = config.clone();
    let exec_cfg = config;
    let g = TaGraphBuilder::<ProductState, ProductState>::overwrite()
        .with_graph_id("plan_execute_review")
        .add_node("plan", move |s: ProductState, c| {
            let cfg = plan_cfg.clone();
            async move {
                run_agent_node(
                    s,
                    &c,
                    cfg,
                    "planner",
                    "task",
                    "plan",
                    "Break this task into an ordered plan:\n\n{input}",
                )
                .await
            }
        })
        .add_node("execute", move |s: ProductState, c| {
            let cfg = exec_cfg.clone();
            async move {
                run_agent_node(
                    s,
                    &c,
                    cfg,
                    "code_executor",
                    "plan",
                    "result",
                    "Execute this plan and report the result:\n\n{input}",
                )
                .await
            }
        })
        .add_node("review", review_node)
        .add_node("finalize", finalize_node)
        .set_entry("plan")
        .add_edge("plan", "execute")
        .add_edge("execute", "review")
        .mark_command_routing("review")
        .set_finish("finalize");
    g.compile()
        .map_err(|e| anyhow::anyhow!("plan_execute_review compile: {e}"))
}

/// `demo_review` — deterministic (no-LLM) twin of `plan_execute_review` for
/// exercising routing, HITL pause/resume, and checkpointing.
pub fn build_demo_review() -> anyhow::Result<ProductGraph> {
    let g = TaGraphBuilder::<ProductState, ProductState>::overwrite()
        .with_graph_id("demo_review")
        .add_node("plan", |mut s: ProductState, _c| async move {
            let input = s.var_str("task");
            s.record_step("plan", format!("PLAN for: {input}"));
            Ok(TaNodeResult::Update(s))
        })
        .add_node("execute", |mut s: ProductState, _c| async move {
            let input = s.var_str("plan");
            s.record_step("result", format!("RESULT of: {input}"));
            Ok(TaNodeResult::Update(s))
        })
        .add_node("review", review_node)
        .add_node("finalize", finalize_node)
        .set_entry("plan")
        .add_edge("plan", "execute")
        .add_edge("execute", "review")
        .mark_command_routing("review")
        .set_finish("finalize");
    g.compile()
        .map_err(|e| anyhow::anyhow!("demo_review compile: {e}"))
}

/// Build a registered product graph by name (tinyagents engine).
pub fn build_tinyagents(name: &str, config: Arc<Config>) -> Result<ProductGraph, String> {
    match name {
        "canonical_turn" => build_canonical_turn(),
        "plan_execute_review" => build_plan_execute_review(config),
        "demo_review" => build_demo_review(),
        other => return Err(format!("unknown graph definition '{other}'")),
    }
    .map_err(|e| format!("compile graph '{name}': {e}"))
}

/// Run an archetype against a templated prompt and record its output (the
/// tinyagents analogue of the in-house `AgentNode`).
async fn run_agent_node(
    mut state: ProductState,
    ctx: &TaNodeContext,
    config: Arc<Config>,
    agent_id: &str,
    input_var: &str,
    output_var: &str,
    prompt_template: &str,
) -> tinyagents::Result<TaNodeResult<ProductState>> {
    let input = state.var_str(input_var);
    let prompt = prompt_template.replace("{input}", &input);
    tracing::info!(
        run_id = %ctx.run_id,
        agent_id,
        output_var,
        "[agent_graph::tinyagents] AgentNode running archetype"
    );
    let mut agent = Agent::from_config_for_agent(&config, agent_id).map_err(|e| {
        tinyagents::TinyAgentsError::Graph(format!("build agent '{agent_id}': {e}"))
    })?;
    let output = agent.run_single(&prompt).await.map_err(|e| {
        tinyagents::TinyAgentsError::Graph(format!("agent '{agent_id}' run failed: {e}"))
    })?;
    state.record_step(output_var, output);
    Ok(TaNodeResult::Update(state))
}

/// HITL review gate: pauses for approval, then routes on the human decision
/// (`approve` → finalize, else → execute). `auto_approve` skips the pause.
fn review_node(
    mut state: ProductState,
    ctx: TaNodeContext,
) -> impl std::future::Future<Output = tinyagents::Result<TaNodeResult<ProductState>>> + Send {
    async move {
        if let Some(decision) = resume_decision(&ctx, &mut state) {
            state.set_var("review_decision", decision.clone());
            tracing::info!(run_id = %ctx.run_id, decision = %decision, "[agent_graph::tinyagents] review resumed");
            let target = if decision.eq_ignore_ascii_case("approve")
                || decision.eq_ignore_ascii_case("yes")
            {
                "finalize"
            } else {
                "execute"
            };
            return Ok(TaNodeResult::Command(
                TaCommand::goto([target]).with_update(state),
            ));
        }

        if state
            .vars
            .get("auto_approve")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            state.set_var("review_decision", "approve");
            return Ok(TaNodeResult::Command(
                TaCommand::goto(["finalize"]).with_update(state),
            ));
        }

        let artifact = state.var_str("result");
        Ok(TaNodeResult::Interrupt(TaInterrupt {
            id: "review-approval".to_string(),
            node: "review".into(),
            payload: serde_json::json!({
                "kind": "approval",
                "prompt": format!("Approve this result?\n\n{artifact}"),
                "options": ["approve", "reject"],
                "resume_to": "review",
            }),
        }))
    }
}

/// Assemble the final answer and end the run.
fn finalize_node(
    mut state: ProductState,
    _ctx: TaNodeContext,
) -> impl std::future::Future<Output = tinyagents::Result<TaNodeResult<ProductState>>> + Send {
    async move {
        let result = state.var_str("result");
        let final_text = format!("Final result:\n{result}");
        state.set_var("final", final_text.clone());
        state.record_step("finalize", final_text);
        Ok(TaNodeResult::Update(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tinyagents::harness::ids::ExecutionStatus;
    use tinyagents::{Checkpointer, InMemoryCheckpointer};

    #[tokio::test]
    async fn canonical_turn_loops_then_finalizes_on_tinyagents() {
        let graph = build_canonical_turn().expect("compile");
        let mut init = ProductState::default();
        init.vars
            .insert("max_iterations".to_string(), Value::from(3));
        let run = graph.run(init).await.expect("run");
        assert_eq!(run.status.status, ExecutionStatus::Completed);
        assert_eq!(run.state.var_str("final"), "completed after 3 iteration(s)");
        let tool_steps = run.state.steps.iter().filter(|s| s.node == "tools").count();
        assert_eq!(tool_steps, 2);
    }

    #[tokio::test]
    async fn demo_review_auto_approve_runs_to_completion() {
        let graph = build_demo_review().expect("compile");
        let mut state = ProductState::from_input(json!({"auto_approve": true}));
        state.set_var("task", "ship feature");
        let run = graph.run(state).await.expect("run");
        assert_eq!(run.status.status, ExecutionStatus::Completed);
        assert_eq!(run.state.var_str("plan"), "PLAN for: ship feature");
        assert_eq!(
            run.state.var_str("result"),
            "RESULT of: PLAN for: ship feature"
        );
        assert_eq!(run.state.var_str("review_decision"), "approve");
        assert!(run.state.var_str("final").starts_with("Final result:"));
    }

    #[tokio::test]
    async fn demo_review_pauses_at_hitl_then_resumes_approve() {
        let checkpointer = Arc::new(InMemoryCheckpointer::<ProductState>::new());
        let graph = build_demo_review()
            .expect("compile")
            .with_checkpointer(checkpointer.clone());
        let state = ProductState::from_input(json!({"task": "ship feature"}));

        let paused = graph.run_with_thread("r1", state).await.expect("run");
        assert_eq!(paused.status.status, ExecutionStatus::Interrupted);
        let interrupt = paused.interrupts.first().expect("interrupt");
        assert_eq!(interrupt.node.as_str(), "review");
        assert_eq!(interrupt.payload["kind"], "approval");

        let done = graph
            .resume("r1", TaCommand::resume(json!("approve")))
            .await
            .expect("resume");
        assert_eq!(done.status.status, ExecutionStatus::Completed);
        assert_eq!(done.state.var_str("review_decision"), "approve");
        assert!(done.state.vars.contains_key("final"));
    }

    #[tokio::test]
    async fn demo_review_reject_loops_back_to_execute() {
        let checkpointer = Arc::new(InMemoryCheckpointer::<ProductState>::new());
        let graph = build_demo_review()
            .expect("compile")
            .with_checkpointer(checkpointer.clone());
        let state = ProductState::from_input(json!({"task": "ship feature"}));

        let paused = graph.run_with_thread("r2", state).await.expect("run");
        assert_eq!(paused.status.status, ExecutionStatus::Interrupted);

        // Reject → loop back to execute → review again (pauses again).
        let repaused = graph
            .resume("r2", TaCommand::resume(json!("reject")))
            .await
            .expect("resume");
        assert_eq!(repaused.status.status, ExecutionStatus::Interrupted);
        assert_eq!(repaused.state.var_str("review_decision"), "reject");
        let _ = checkpointer.list("r2").await;
    }

    #[test]
    fn build_tinyagents_rejects_unknown() {
        assert!(build_tinyagents("ghost", Arc::new(Config::default())).is_err());
    }
}
