//! LangGraph-compatible execution chain for the `subconscious` built-in agent.
//!
//! Sits next to `prompt.rs`: `prompt.rs` defines what the agent *says*,
//! `graph.rs` defines how it *runs* — the node/edge chain the `agent_graph`
//! engine drives. See [`crate::openhuman::agent_graph::blueprint`].

use crate::openhuman::agent_graph::blueprint::{self, GraphBlueprint};

/// The `subconscious` agent's execution chain.
pub fn graph() -> GraphBlueprint {
    blueprint::single_shot("subconscious")
}
