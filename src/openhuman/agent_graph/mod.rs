//! # `agent_graph` — LangGraph-style state-machine harness (issue #4249)
//!
//! Replaces the implicit "next tool call" loop with an explicit directed graph
//! of nodes (states) and edges (transitions), modelled on
//! [LangGraph](https://langchain-ai.github.io/langgraph/) and the
//! [`rust-langgraph`](https://docs.rs/rust-langgraph) crate:
//!
//! - **Explicit state graph** — [`graph::StateGraph`] builder (`add_node`,
//!   `add_edge`, `add_conditional_edges`, `set_entry_point`,
//!   `set_finish_point`, `compile`) compiled into a [`graph::CompiledGraph`] run
//!   by a Pregel-style super-step executor.
//! - **Persistent working state** — a typed [`graph::GraphState`] (with a
//!   `merge` reducer) that survives node transitions, parallel branches, and
//!   checkpoints.
//! - **Branching & merging** — conditional edges + fork/merge super-steps.
//! - **Checkpointing** — [`checkpoint`]: a pluggable `Checkpointer`
//!   (in-memory + SQLite) for pause/resume of long-running runs.
//! - **Human-in-the-loop** — [`hitl`]: nodes that pause the run via
//!   `Command::Interrupt` and resume on external input.
//! - **Observability / summarization / memory hooks** — [`observability`],
//!   [`summarization`], [`memory`] compose the existing harness primitives
//!   rather than reinventing them.
//!
//! Concrete graphs (the canonical agent turn + the `plan_execute_review`
//! product graph) live in [`definitions`]. The RPC surface
//! (`openhuman.agent_graph_*`) lives in [`schemas`] + [`ops`].

pub mod blueprint;
pub mod checkpoint;
pub mod definitions;
pub mod hitl;
pub mod memory;
pub mod observability;
pub mod summarization;
pub mod tinyagents;
pub mod types;

pub mod ops;
mod schemas;

pub use schemas::{
    all_controller_schemas as all_agent_graph_controller_schemas,
    all_registered_controllers as all_agent_graph_registered_controllers,
};
