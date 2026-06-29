//! LangGraph-compatible execution chain for the `account_admin_agent` built-in agent.
//!
//! Sits next to `prompt.rs`: `prompt.rs` defines what the agent *says*,
//! `graph.rs` defines how it *runs* — the node/edge chain the `agent_graph`
//! engine drives. See [`crate::openhuman::agent_graph::blueprint`].

use crate::openhuman::agent_graph::blueprint::{self, GraphBlueprint};

/// The `account_admin_agent` agent's execution chain.
pub fn graph() -> GraphBlueprint {
    blueprint::canonical_turn("account_admin_agent")
}
