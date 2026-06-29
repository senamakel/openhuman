//! The canonical agent turn, expressed as an explicit state graph.
//!
//! This is the structural replacement for the implicit "next tool call" loop in
//! [`run_turn_engine`](crate::openhuman::agent::harness): the control flow that
//! was a `for iteration in 0..max` with inline branching becomes named nodes and
//! edges —
//!
//! ```text
//! entry → dispatch → parse ──(no tool calls)──────────────→ finalize → END
//!            ▲                └─(tool calls)→ stop_check ─(stop)→ finalize → END
//!            │                                   │
//!            └────────── compact ← tools ◄───────┘ (continue)
//! ```
//!
//! The node bodies here drive a bounded counter so the graph is runnable and
//! testable as a faithful model of the loop's shape (iteration cap → finalize).
//! Wiring each node to the production provider / tool / context primitives
//! (`ToolSource`, `ResponseParser`, `ContextManager`, `StopHook`) is the
//! integration step that routes the live turn through this graph; the topology
//! and routing semantics are defined and exercised here.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use super::state::ProductState;
use crate::openhuman::agent_graph::graph::{Node, NodeCtx, NodeOutput, StateGraph};
use crate::openhuman::agent_graph::types::GraphError;

/// `vars["max_iterations"]` cap (default 10, mirroring the harness default).
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

/// Provider-call node — advances the iteration counter (stand-in for the LLM
/// dispatch the production wiring performs).
struct Dispatch;
#[async_trait]
impl Node<ProductState> for Dispatch {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        let next = iter_count(&s) + 1;
        s.vars.insert("__iter".to_string(), Value::from(next));
        Ok(NodeOutput::cont(s))
    }
}

/// Parse node — decides "tool calls vs final answer". Modelled as: keep calling
/// tools until the iteration cap, then produce the final answer.
struct Parse;
#[async_trait]
impl Node<ProductState> for Parse {
    async fn run(&self, s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        if iter_count(&s) >= max_iters(&s) {
            Ok(NodeOutput::goto(s, "finalize"))
        } else {
            Ok(NodeOutput::goto(s, "stop_check"))
        }
    }
}

/// Stop-hook node — policy gate (budget/iteration). Here: stop at the cap.
struct StopCheck;
#[async_trait]
impl Node<ProductState> for StopCheck {
    async fn run(&self, s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        if iter_count(&s) >= max_iters(&s) {
            Ok(NodeOutput::goto(s, "finalize"))
        } else {
            Ok(NodeOutput::cont(s)) // → tools
        }
    }
}

/// Tool-execution node (the production wiring fans tool calls out here).
struct Tools;
#[async_trait]
impl Node<ProductState> for Tools {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        let n = iter_count(&s);
        s.record_step("tools", format!("executed tools (iteration {n})"));
        Ok(NodeOutput::cont(s)) // → compact
    }
}

/// Context-management node (microcompact/autocompact).
struct Compact;
#[async_trait]
impl Node<ProductState> for Compact {
    async fn run(&self, s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        Ok(NodeOutput::goto(s, "dispatch")) // loop back
    }
}

/// Final-answer node.
struct Finalize;
#[async_trait]
impl Node<ProductState> for Finalize {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        let n = iter_count(&s);
        s.set_var("final", format!("completed after {n} iteration(s)"));
        s.record_step("finalize", format!("final answer after {n} iteration(s)"));
        Ok(NodeOutput::end(s))
    }
}

/// Node ids of the canonical turn, in topological reading order.
pub const NODES: &[&str] = &[
    "dispatch",
    "parse",
    "stop_check",
    "tools",
    "compact",
    "finalize",
];

/// Build the canonical-turn graph.
pub fn build(
) -> Result<crate::openhuman::agent_graph::graph::CompiledGraph<ProductState>, GraphError> {
    let mut g = StateGraph::<ProductState>::new("canonical_turn");
    g.add_node("dispatch", Arc::new(Dispatch));
    g.add_node("parse", Arc::new(Parse));
    g.add_node("stop_check", Arc::new(StopCheck));
    g.add_node("tools", Arc::new(Tools));
    g.add_node("compact", Arc::new(Compact));
    g.add_node("finalize", Arc::new(Finalize));
    g.set_entry_point("dispatch");
    // Static spine; nodes override with Goto where they branch/loop.
    g.add_edge("dispatch", "parse");
    g.add_edge("parse", "stop_check");
    g.add_edge("stop_check", "tools");
    g.add_edge("tools", "compact");
    g.add_edge("compact", "dispatch");
    g.set_finish_point("finalize");
    g.compile()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn canonical_turn_loops_then_finalizes() {
        let graph = build().expect("compile");
        let mut init = ProductState::default();
        init.vars
            .insert("max_iterations".to_string(), Value::from(3));
        let out = graph.invoke(init).await.expect("run");
        assert_eq!(
            out.status,
            crate::openhuman::agent_graph::types::GraphRunStatus::Completed
        );
        assert_eq!(out.state.var_str("final"), "completed after 3 iteration(s)");
        // With cap=3, the 3rd dispatch routes straight to finalize (final answer,
        // no tool call), so tools run on iterations 1 and 2.
        let tool_steps = out.state.steps.iter().filter(|s| s.node == "tools").count();
        assert_eq!(tool_steps, 2);
    }
}
