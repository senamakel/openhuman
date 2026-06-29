//! Edges: how the executor routes out of a node when it returns
//! [`Command::Continue`](crate::openhuman::agent_graph::types::Command::Continue).

use super::state::GraphState;
use crate::openhuman::agent_graph::types::NodeId;

/// A router for a conditional edge: inspects the current state and names the
/// next node. Boxed so the builder can store heterogeneous closures.
pub type Router<S> = Box<dyn Fn(&S) -> NodeId + Send + Sync>;

/// The statically-declared successor relationship for a node.
pub enum Edge<S: GraphState> {
    /// Always route to one node.
    Static(NodeId),
    /// Pick the next node by inspecting state (LangGraph `add_conditional_edges`).
    Conditional(Router<S>),
    /// Fan out to several nodes in parallel.
    Fork(Vec<NodeId>),
}

impl<S: GraphState> Edge<S> {
    /// Every node id this edge can statically reach. Used by `compile()` to
    /// validate targets (conditional edges declare their reachable set so the
    /// validator can still check them).
    pub fn declared_targets(&self) -> Vec<NodeId> {
        match self {
            Edge::Static(n) => vec![n.clone()],
            // Conditional routers are opaque to static validation; their targets
            // are validated at runtime when the router returns an id. The
            // builder pairs them with an explicit reachable-set in
            // `add_conditional_edges` (stored as `Fork`-like metadata) — see
            // builder.rs.
            Edge::Conditional(_) => vec![],
            Edge::Fork(ns) => ns.clone(),
        }
    }
}
