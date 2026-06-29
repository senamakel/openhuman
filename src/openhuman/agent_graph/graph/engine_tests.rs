//! Unit tests for the generic state-graph engine — no agent-harness coupling.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{GraphCancel, GraphState, Node, NodeCtx, NodeOutput, NullSink, StateGraph};
use crate::openhuman::agent_graph::types::{Command, GraphError, GraphRunStatus, InterruptRequest};

/// A tiny state: a visit log + a counter, so tests can assert on order and
/// merge semantics.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct TestState {
    log: Vec<String>,
    counter: i64,
    branch: String,
}

impl GraphState for TestState {
    fn merge(&mut self, other: Self) -> Result<()> {
        // List-append reducer for `log`, sum for `counter`, keep first branch.
        self.log.extend(other.log);
        self.counter += other.counter;
        if self.branch.is_empty() {
            self.branch = other.branch;
        }
        Ok(())
    }
}

/// Node that appends its name to the log and bumps the counter.
struct Append(&'static str);
#[async_trait]
impl Node<TestState> for Append {
    async fn run(&self, mut state: TestState, _ctx: &NodeCtx<'_>) -> Result<NodeOutput<TestState>> {
        state.log.push(self.0.to_string());
        state.counter += 1;
        Ok(NodeOutput::cont(state))
    }
}

/// Node that loops back to "work" until counter >= 3, then ends.
struct LoopWork;
#[async_trait]
impl Node<TestState> for LoopWork {
    async fn run(&self, mut state: TestState, _ctx: &NodeCtx<'_>) -> Result<NodeOutput<TestState>> {
        state.counter += 1;
        state.log.push(format!("work{}", state.counter));
        if state.counter >= 3 {
            Ok(NodeOutput::goto(state, "done"))
        } else {
            Ok(NodeOutput::goto(state, "work"))
        }
    }
}

struct Done;
#[async_trait]
impl Node<TestState> for Done {
    async fn run(&self, mut state: TestState, _ctx: &NodeCtx<'_>) -> Result<NodeOutput<TestState>> {
        state.log.push("done".to_string());
        Ok(NodeOutput::end(state))
    }
}

/// Branch node that tags which branch it is and ends.
struct BranchTag(&'static str);
#[async_trait]
impl Node<TestState> for BranchTag {
    async fn run(&self, mut state: TestState, _ctx: &NodeCtx<'_>) -> Result<NodeOutput<TestState>> {
        state.log.push(format!("branch:{}", self.0));
        state.counter += 1;
        Ok(NodeOutput::end(state))
    }
}

/// Node that interrupts for human input on first pass; on resume the state's
/// `branch` is already set so it falls through.
struct HitlNode;
#[async_trait]
impl Node<TestState> for HitlNode {
    async fn run(&self, mut state: TestState, _ctx: &NodeCtx<'_>) -> Result<NodeOutput<TestState>> {
        state.log.push("hitl".to_string());
        Ok(NodeOutput {
            state,
            command: Command::Interrupt(InterruptRequest {
                kind: "approval".to_string(),
                question: "approve?".to_string(),
                options: vec!["yes".to_string(), "no".to_string()],
                resume_to: None,
            }),
        })
    }
}

fn sink() -> NullSink {
    NullSink
}

/// `compile()` returns a `CompiledGraph` that holds trait objects and so isn't
/// `Debug`; `.unwrap_err()` would require the Ok type to be `Debug`. This helper
/// extracts the error without that bound.
fn expect_compile_err(result: Result<super::CompiledGraph<TestState>, GraphError>) -> GraphError {
    match result {
        Ok(_) => panic!("expected compile error"),
        Err(e) => e,
    }
}

#[tokio::test]
async fn linear_chain_runs_in_order() {
    let mut g = StateGraph::<TestState>::new("linear");
    g.add_node("a", Arc::new(Append("a")));
    g.add_node("b", Arc::new(Append("b")));
    g.add_node("c", Arc::new(Append("c")));
    g.set_entry_point("a");
    g.add_edge("a", "b");
    g.add_edge("b", "c");
    g.set_finish_point("c");
    let compiled = g.compile().expect("compile");

    let out = compiled.invoke(TestState::default()).await.expect("run");
    assert_eq!(out.status, GraphRunStatus::Completed);
    assert_eq!(out.state.log, vec!["a", "b", "c"]);
    assert_eq!(out.state.counter, 3);
    assert_eq!(out.last_node, "c");
    // 3 nodes ran sequentially → 3 super-steps.
    assert_eq!(out.steps, 3);
    assert_eq!(out.transitions.len(), 3);
}

#[tokio::test]
async fn conditional_edge_routes_on_state() {
    let mut g = StateGraph::<TestState>::new("cond");
    g.add_node("start", Arc::new(Append("start")));
    g.add_node("even", Arc::new(BranchTag("even")));
    g.add_node("odd", Arc::new(BranchTag("odd")));
    g.set_entry_point("start");
    g.add_conditional_edges(
        "start",
        vec!["even".to_string(), "odd".to_string()],
        Box::new(|s: &TestState| {
            if s.counter % 2 == 0 {
                "even".to_string()
            } else {
                "odd".to_string()
            }
        }),
    );
    g.set_finish_point("even");
    g.set_finish_point("odd");
    let compiled = g.compile().expect("compile");

    // counter starts 0, start bumps to 1 → odd.
    let out = compiled.invoke(TestState::default()).await.expect("run");
    assert_eq!(out.state.log, vec!["start", "branch:odd"]);
}

#[tokio::test]
async fn cycle_loops_until_condition() {
    let mut g = StateGraph::<TestState>::new("cycle");
    g.add_node("work", Arc::new(LoopWork));
    g.add_node("done", Arc::new(Done));
    g.set_entry_point("work");
    g.add_edge("work", "work"); // self-loop fallthrough (unused; node uses Goto)
    g.add_edge("done", crate::openhuman::agent_graph::types::END);
    let compiled = g.compile().expect("compile");

    let out = compiled.invoke(TestState::default()).await.expect("run");
    assert_eq!(out.status, GraphRunStatus::Completed);
    assert_eq!(out.state.counter, 3);
    assert_eq!(out.state.log, vec!["work1", "work2", "work3", "done"]);
}

#[tokio::test]
async fn fork_runs_branches_and_merges() {
    let mut g = StateGraph::<TestState>::new("fork");
    g.add_node("split", Arc::new(Append("split")));
    g.add_node("left", Arc::new(BranchTag("left")));
    g.add_node("right", Arc::new(BranchTag("right")));
    g.set_entry_point("split");
    g.add_fork("split", vec!["left".to_string(), "right".to_string()]);
    g.set_finish_point("left");
    g.set_finish_point("right");
    let compiled = g.compile().expect("compile");

    let out = compiled.invoke(TestState::default()).await.expect("run");
    assert_eq!(out.status, GraphRunStatus::Completed);
    // Each branch receives a CLONE of the full pre-fork state (counter 1) and
    // adds 1 → branch states counter 2 each. `merge` (sum reducer) folds them →
    // 4. The shared "split" prefix therefore appears in both branches' logs.
    assert_eq!(out.state.counter, 4);
    assert!(out.state.log.contains(&"branch:left".to_string()));
    assert!(out.state.log.contains(&"branch:right".to_string()));
    // Deterministic merge order (branches keyed by node id: left before right).
    assert_eq!(
        out.state.log,
        vec!["split", "branch:left", "split", "branch:right"]
    );
}

#[tokio::test]
async fn interrupt_pauses_and_resume_completes() {
    let mut g = StateGraph::<TestState>::new("hitl");
    g.add_node("hitl", Arc::new(HitlNode));
    g.add_node("after", Arc::new(Append("after")));
    g.set_entry_point("hitl");
    g.add_edge("hitl", "after");
    g.set_finish_point("after");
    let compiled = g.compile().expect("compile");

    let run_id = "run-1";
    let cancel = GraphCancel::new();
    let paused = compiled
        .invoke_with(TestState::default(), run_id, &sink(), &cancel)
        .await
        .expect("run");
    assert_eq!(paused.status, GraphRunStatus::Paused);
    let interrupt = paused.interrupt.expect("interrupt");
    assert_eq!(interrupt.kind, "approval");
    // resume_to defaults to the static successor of the interrupting node.
    assert_eq!(interrupt.resume_to.as_deref(), Some("after"));

    // Resume from the recorded node with the (human-updated) state.
    let mut state = paused.state;
    state.branch = "approved".to_string();
    let resumed = compiled
        .resume_with(
            state,
            interrupt.resume_to.unwrap(),
            run_id,
            &sink(),
            &cancel,
            paused.steps,
        )
        .await
        .expect("resume");
    assert_eq!(resumed.status, GraphRunStatus::Completed);
    assert_eq!(resumed.state.log, vec!["hitl", "after"]);
    assert_eq!(resumed.state.branch, "approved");
}

#[tokio::test]
async fn compile_rejects_missing_entry() {
    let mut g = StateGraph::<TestState>::new("bad");
    g.add_node("a", Arc::new(Append("a")));
    g.set_finish_point("a");
    let err = expect_compile_err(g.compile());
    assert!(matches!(err, GraphError::NoEntryPoint));
}

#[tokio::test]
async fn compile_rejects_unknown_edge_target() {
    let mut g = StateGraph::<TestState>::new("bad");
    g.add_node("a", Arc::new(Append("a")));
    g.set_entry_point("a");
    g.add_edge("a", "ghost");
    let err = expect_compile_err(g.compile());
    assert!(matches!(err, GraphError::UnknownNode(n) if n == "ghost"));
}

#[tokio::test]
async fn compile_rejects_dangling_node() {
    let mut g = StateGraph::<TestState>::new("bad");
    g.add_node("a", Arc::new(Append("a")));
    g.set_entry_point("a");
    // "a" has no edge and is not a finish point → dangling.
    let err = expect_compile_err(g.compile());
    assert!(matches!(err, GraphError::DanglingNode(n) if n == "a"));
}

#[tokio::test]
async fn step_cap_trips_on_runaway_cycle() {
    struct Spin;
    #[async_trait]
    impl Node<TestState> for Spin {
        async fn run(
            &self,
            mut state: TestState,
            _ctx: &NodeCtx<'_>,
        ) -> Result<NodeOutput<TestState>> {
            state.counter += 1;
            Ok(NodeOutput::goto(state, "spin"))
        }
    }
    let mut g = StateGraph::<TestState>::new("spin");
    g.add_node("spin", Arc::new(Spin));
    g.set_entry_point("spin");
    g.add_edge("spin", "spin");
    g.set_max_steps(5);
    let compiled = g.compile().expect("compile");
    let err = compiled.invoke(TestState::default()).await.unwrap_err();
    assert!(matches!(err, GraphError::MaxStepsExceeded(5)));
}

#[tokio::test]
async fn cancel_aborts_run() {
    let mut g = StateGraph::<TestState>::new("cancellable");
    g.add_node("a", Arc::new(Append("a")));
    g.set_entry_point("a");
    g.add_edge("a", "a");
    let compiled = g.compile().expect("compile");
    let cancel = GraphCancel::new();
    cancel.cancel();
    let err = compiled
        .invoke_with(TestState::default(), "r", &sink(), &cancel)
        .await
        .unwrap_err();
    assert!(matches!(err, GraphError::Cancelled));
}

#[tokio::test]
async fn node_error_propagates_with_node_name() {
    struct Boom;
    #[async_trait]
    impl Node<TestState> for Boom {
        async fn run(&self, _s: TestState, _c: &NodeCtx<'_>) -> Result<NodeOutput<TestState>> {
            anyhow::bail!("kaboom")
        }
    }
    let mut g = StateGraph::<TestState>::new("boom");
    g.add_node("boom", Arc::new(Boom));
    g.set_entry_point("boom");
    g.set_finish_point("boom");
    let compiled = g.compile().expect("compile");
    let err = compiled.invoke(TestState::default()).await.unwrap_err();
    assert!(matches!(err, GraphError::NodeFailed { node, .. } if node == "boom"));
}
