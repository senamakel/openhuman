//! Node ids of the built-in product graphs, for `agent_graph_definition_list`.
//!
//! The runnable graphs live in [`super::tinyagents_graphs`]
//! (`build_plan_execute_review` / `build_demo_review`), executed on the
//! `tinyagents` engine. This module now only exposes the shared node-id list the
//! RPC definition registry advertises.

/// Shared node ids for `plan_execute_review` / `demo_review`.
pub const NODES: &[&str] = &["plan", "execute", "review", "finalize"];
