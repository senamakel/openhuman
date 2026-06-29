//! The generic LangGraph-style state-machine engine.
//!
//! Intentionally free of agent-harness coupling so it can be unit-tested in
//! isolation. The agent integration lives in
//! [`crate::openhuman::agent_graph::definitions`].
//!
//! ```ignore
//! let mut g = StateGraph::<MyState>::new("demo");
//! g.add_node("a", Arc::new(NodeA));
//! g.add_node("b", Arc::new(NodeB));
//! g.set_entry_point("a");
//! g.add_edge("a", "b");
//! g.set_finish_point("b");
//! let compiled = g.compile()?;
//! let result = compiled.invoke(MyState::default()).await?;
//! ```

mod builder;
mod compiled;
mod edge;
mod executor;
mod node;
mod state;

pub use builder::{CompiledGraph, StateGraph, DEFAULT_MAX_STEPS};
pub use edge::{Edge, Router};
pub use node::{GraphCancel, Node, NodeCtx, NodeOutput, NullSink, ProgressSink};
pub use state::GraphState;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod engine_tests;
