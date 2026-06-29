//! The `plan_execute_review` product graph (+ a deterministic `demo_review`
//! twin for tests).
//!
//! Topology (shared node names so [`ReviewGate`] can route by id):
//!
//! ```text
//! plan → execute → review ──(approve)──→ finalize → END
//!          ▲                 └─(reject)──┘ (loop back to execute)
//!          │                    (HITL interrupt pauses here)
//! ```

use std::sync::Arc;

use super::nodes::{AgentNode, FinalizeNode, ReviewGate, StubNode};
use super::state::ProductState;
use crate::openhuman::agent_graph::graph::{CompiledGraph, StateGraph};
use crate::openhuman::agent_graph::types::GraphError;
use crate::openhuman::config::Config;

/// Shared node ids.
pub const NODES: &[&str] = &["plan", "execute", "review", "finalize"];

fn wire(g: &mut StateGraph<ProductState>) {
    g.set_entry_point("plan");
    g.add_edge("plan", "execute");
    g.add_edge("execute", "review");
    // review routes via Goto (finalize/execute); declare a fallthrough so the
    // node is never dangling.
    g.add_edge("review", "finalize");
    g.set_finish_point("finalize");
}

/// `plan_execute_review` — composes the existing `planner` and `code_executor`
/// archetypes around a human review gate.
pub fn build_plan_execute_review(
    config: Arc<Config>,
) -> Result<CompiledGraph<ProductState>, GraphError> {
    let mut g = StateGraph::<ProductState>::new("plan_execute_review");
    g.add_node(
        "plan",
        Arc::new(AgentNode {
            agent_id: "planner".to_string(),
            input_var: "task".to_string(),
            output_var: "plan".to_string(),
            prompt_template: "Break this task into an ordered plan:\n\n{input}".to_string(),
            config: config.clone(),
        }),
    );
    g.add_node(
        "execute",
        Arc::new(AgentNode {
            agent_id: "code_executor".to_string(),
            input_var: "plan".to_string(),
            output_var: "result".to_string(),
            prompt_template: "Execute this plan and report the result:\n\n{input}".to_string(),
            config,
        }),
    );
    g.add_node(
        "review",
        Arc::new(ReviewGate {
            review_var: "result".to_string(),
        }),
    );
    g.add_node(
        "finalize",
        Arc::new(FinalizeNode {
            result_var: "result".to_string(),
        }),
    );
    wire(&mut g);
    g.compile()
}

/// `demo_review` — same topology, deterministic (no LLM). Used by unit/E2E tests
/// to exercise routing, HITL pause/resume, and checkpointing without a backend.
pub fn build_demo_review() -> Result<CompiledGraph<ProductState>, GraphError> {
    let mut g = StateGraph::<ProductState>::new("demo_review");
    g.add_node(
        "plan",
        Arc::new(StubNode {
            input_var: "task".to_string(),
            output_var: "plan".to_string(),
            template: "PLAN for: {input}".to_string(),
        }),
    );
    g.add_node(
        "execute",
        Arc::new(StubNode {
            input_var: "plan".to_string(),
            output_var: "result".to_string(),
            template: "RESULT of: {input}".to_string(),
        }),
    );
    g.add_node(
        "review",
        Arc::new(ReviewGate {
            review_var: "result".to_string(),
        }),
    );
    g.add_node(
        "finalize",
        Arc::new(FinalizeNode {
            result_var: "result".to_string(),
        }),
    );
    wire(&mut g);
    g.compile()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::agent_graph::types::GraphRunStatus;
    use serde_json::json;

    #[tokio::test]
    async fn demo_review_auto_approve_runs_to_completion() {
        let graph = build_demo_review().expect("compile");
        let mut state = ProductState::from_input(json!({"task": "x", "auto_approve": true}));
        state.set_var("task", "ship feature");
        let out = graph.invoke(state).await.expect("run");
        assert_eq!(out.status, GraphRunStatus::Completed);
        assert_eq!(out.state.var_str("plan"), "PLAN for: ship feature");
        assert_eq!(
            out.state.var_str("result"),
            "RESULT of: PLAN for: ship feature"
        );
        assert_eq!(out.state.var_str("review_decision"), "approve");
        assert!(out.state.var_str("final").starts_with("Final result:"));
    }

    #[tokio::test]
    async fn demo_review_pauses_at_hitl_then_resumes_approve() {
        let graph = build_demo_review().expect("compile");
        let state = ProductState::from_input(json!({"task": "ship feature"}));
        let paused = graph.invoke(state).await.expect("run");
        assert_eq!(paused.status, GraphRunStatus::Paused);
        let interrupt = paused.interrupt.clone().expect("interrupt");
        assert_eq!(interrupt.kind, "approval");
        assert_eq!(interrupt.resume_to.as_deref(), Some("review"));

        // Resume with approval.
        let mut resumed_state = paused.state;
        resumed_state.resume_input = Some("approve".to_string());
        let done = graph
            .resume_with(
                resumed_state,
                "review".to_string(),
                "r1",
                &crate::openhuman::agent_graph::graph::NullSink,
                &crate::openhuman::agent_graph::graph::GraphCancel::new(),
                paused.steps,
            )
            .await
            .expect("resume");
        assert_eq!(done.status, GraphRunStatus::Completed);
        assert_eq!(done.state.var_str("review_decision"), "approve");
        assert!(done.state.vars.contains_key("final"));
    }

    #[tokio::test]
    async fn demo_review_reject_loops_back_to_execute() {
        let graph = build_demo_review().expect("compile");
        let state = ProductState::from_input(json!({"task": "ship feature"}));
        let paused = graph.invoke(state).await.expect("run");
        let mut resumed_state = paused.state;
        resumed_state.resume_input = Some("reject".to_string());
        // Resume into review; reject → execute → review again (pauses again).
        let repaused = graph
            .resume_with(
                resumed_state,
                "review".to_string(),
                "r1",
                &crate::openhuman::agent_graph::graph::NullSink,
                &crate::openhuman::agent_graph::graph::GraphCancel::new(),
                paused.steps,
            )
            .await
            .expect("resume");
        assert_eq!(repaused.status, GraphRunStatus::Paused);
        assert_eq!(repaused.state.var_str("review_decision"), "reject");
    }
}
