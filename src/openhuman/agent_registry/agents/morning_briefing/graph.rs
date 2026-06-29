//! LangGraph-compatible execution chain for the `morning_briefing` built-in agent.
//!
//! Sits next to `prompt.rs`: `prompt.rs` defines what the agent *says*,
//! `graph.rs` defines how it *runs* — the node/edge chain the `agent_graph`
//! engine drives. See [`crate::openhuman::agent_graph::blueprint`].

use crate::openhuman::agent_graph::blueprint::{self, GraphBlueprint};

/// The `morning_briefing` agent's execution chain.
pub fn graph() -> GraphBlueprint {
    blueprint::single_shot("morning_briefing")
}
