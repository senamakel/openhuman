//! [`StateGraph`] builder + [`CompiledGraph`] handle.
//!
//! Mirrors LangGraph's builder surface: `add_node`, `add_edge`,
//! `add_conditional_edges`, `set_entry_point`, `set_finish_point`, `compile`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::edge::{Edge, Router};
use super::node::Node;
use super::state::GraphState;
use crate::openhuman::agent_graph::types::{GraphError, NodeId, END};

/// Default cap on super-steps before the executor trips the runaway-cycle
/// guard. Generous enough for a long tool-calling turn (each iteration is a few
/// super-steps) yet bounded.
pub const DEFAULT_MAX_STEPS: u32 = 512;

/// A mutable graph definition. Build it up, then [`compile`](StateGraph::compile)
/// into an immutable [`CompiledGraph`] the executor can run.
pub struct StateGraph<S: GraphState> {
    name: String,
    nodes: HashMap<NodeId, Arc<dyn Node<S>>>,
    edges: HashMap<NodeId, Edge<S>>,
    /// Declared reachable targets for conditional edges, keyed by source node.
    /// Lets `compile()` validate a router's possible destinations even though
    /// the closure itself is opaque.
    conditional_targets: HashMap<NodeId, Vec<NodeId>>,
    entry: Option<NodeId>,
    finish: HashSet<NodeId>,
    max_steps: u32,
}

impl<S: GraphState> StateGraph<S> {
    /// New empty graph with a human-readable `name` (used in logs + run records).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            conditional_targets: HashMap::new(),
            entry: None,
            finish: HashSet::new(),
            max_steps: DEFAULT_MAX_STEPS,
        }
    }

    /// Register a node under `id`. Re-adding the same id replaces it.
    pub fn add_node(&mut self, id: impl Into<NodeId>, node: Arc<dyn Node<S>>) -> &mut Self {
        self.nodes.insert(id.into(), node);
        self
    }

    /// Statically route `from` → `to` after `from` returns `Command::Continue`.
    /// Use the [`END`] constant as `to` to finish the branch.
    pub fn add_edge(&mut self, from: impl Into<NodeId>, to: impl Into<NodeId>) -> &mut Self {
        self.edges.insert(from.into(), Edge::Static(to.into()));
        self
    }

    /// Route `from` by inspecting state. `targets` is the declared reachable set
    /// (validated by `compile`); `router` picks one of them at runtime.
    pub fn add_conditional_edges(
        &mut self,
        from: impl Into<NodeId>,
        targets: Vec<NodeId>,
        router: Router<S>,
    ) -> &mut Self {
        let from = from.into();
        self.conditional_targets.insert(from.clone(), targets);
        self.edges.insert(from, Edge::Conditional(router));
        self
    }

    /// Fan `from` out to every node in `targets` in parallel.
    pub fn add_fork(&mut self, from: impl Into<NodeId>, targets: Vec<NodeId>) -> &mut Self {
        self.edges.insert(from.into(), Edge::Fork(targets));
        self
    }

    /// Set the first node scheduled when the graph runs.
    pub fn set_entry_point(&mut self, id: impl Into<NodeId>) -> &mut Self {
        self.entry = Some(id.into());
        self
    }

    /// Mark `id` as a finish node: when it returns `Command::Continue` (and has
    /// no outgoing edge) the branch terminates instead of dangling.
    pub fn set_finish_point(&mut self, id: impl Into<NodeId>) -> &mut Self {
        self.finish.insert(id.into());
        self
    }

    /// Override the super-step cap (runaway-cycle guard).
    pub fn set_max_steps(&mut self, max: u32) -> &mut Self {
        self.max_steps = max.max(1);
        self
    }

    /// Validate the graph and freeze it into a [`CompiledGraph`].
    ///
    /// Checks: an entry point is set and known; every edge target (static, fork,
    /// or declared-conditional) names a real node or [`END`]; no node both
    /// returns `Continue` and lacks an edge while not being a finish node.
    pub fn compile(self) -> Result<CompiledGraph<S>, GraphError> {
        let entry = self.entry.clone().ok_or(GraphError::NoEntryPoint)?;
        if !self.nodes.contains_key(&entry) {
            return Err(GraphError::UnknownNode(entry));
        }

        let known = |id: &str| id == END || self.nodes.contains_key(id);

        for (from, edge) in &self.edges {
            if !self.nodes.contains_key(from) {
                return Err(GraphError::UnknownNode(from.clone()));
            }
            for target in edge.declared_targets() {
                if !known(&target) {
                    return Err(GraphError::UnknownNode(target));
                }
            }
        }
        for targets in self.conditional_targets.values() {
            for target in targets {
                if !known(target) {
                    return Err(GraphError::UnknownNode(target.clone()));
                }
            }
        }

        // A node that has no outgoing edge must be a finish point, otherwise a
        // `Command::Continue` from it has nowhere to go.
        for id in self.nodes.keys() {
            if !self.edges.contains_key(id) && !self.finish.contains(id) {
                return Err(GraphError::DanglingNode(id.clone()));
            }
        }

        Ok(CompiledGraph {
            name: self.name,
            nodes: self.nodes,
            edges: self.edges,
            entry,
            finish: self.finish,
            max_steps: self.max_steps,
        })
    }
}

/// A validated, immutable graph ready to run. Executed by
/// [`crate::openhuman::agent_graph::graph::executor`].
pub struct CompiledGraph<S: GraphState> {
    pub(crate) name: String,
    pub(crate) nodes: HashMap<NodeId, Arc<dyn Node<S>>>,
    pub(crate) edges: HashMap<NodeId, Edge<S>>,
    pub(crate) entry: NodeId,
    pub(crate) finish: HashSet<NodeId>,
    pub(crate) max_steps: u32,
}

impl<S: GraphState> CompiledGraph<S> {
    /// The graph's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The entry node id.
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// Resolve the static successor of `node` given the current `state`.
    ///
    /// Returns `None` when `node` is a finish node with no edge (branch ends),
    /// or `Some(END)` / `Some(next)` otherwise. Fork edges return their first
    /// member here; the executor handles true fan-out separately via the node's
    /// returned [`Command::Fork`](crate::openhuman::agent_graph::types::Command::Fork).
    pub(crate) fn static_next(&self, node: &str, state: &S) -> Option<NodeId> {
        match self.edges.get(node) {
            Some(Edge::Static(n)) => Some(n.clone()),
            Some(Edge::Conditional(router)) => Some(router(state)),
            Some(Edge::Fork(ns)) => ns.first().cloned(),
            None => {
                if self.finish.contains(node) {
                    Some(END.to_string())
                } else {
                    None
                }
            }
        }
    }

    /// Fork targets statically declared for `node`, if it has a fork edge.
    pub(crate) fn fork_targets(&self, node: &str) -> Option<Vec<NodeId>> {
        match self.edges.get(node) {
            Some(Edge::Fork(ns)) => Some(ns.clone()),
            _ => None,
        }
    }
}
