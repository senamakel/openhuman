//! LangGraph-compatible execution chain for the `planner` built-in agent.
//!
//! Sits next to `prompt.rs`: `prompt.rs` defines what the agent *says*,
//! `graph.rs` defines how it *runs* — the node/edge chain the `agent_graph`
//! engine drives. See [`crate::openhuman::agent_graph::blueprint`].

use crate::openhuman::agent_graph::blueprint::{self, GraphBlueprint};

/// The `planner` agent's execution chain.
pub fn graph() -> GraphBlueprint {
    blueprint::plan_execute_review("planner")
}
