//! Per-agent graph **blueprints** — the declarative, LangGraph-compatible chain
//! each built-in agent defines in its own `graph.rs` (next to `prompt.rs`).
//!
//! A [`GraphBlueprint`] is a serializable description of an agent's execution
//! chain: typed nodes ([`NodeKind`]) and edges ([`EdgeSpec`]) with an entry and
//! finish set. It is:
//!
//! - **Inspectable** — surfaced over RPC so the chain can be visualised.
//! - **Validated** — [`GraphBlueprint::compile`] turns it into a real
//!   [`CompiledGraph`], so a malformed chain fails at build/test time.
//! - **Runnable** — the compiled graph executes on the shared engine, driving
//!   [`ProductState`] through the declared topology.
//!
//! Most agents reuse [`canonical_turn`] (the standard tool-calling loop);
//! specialised agents (orchestrator, planner, critic, …) declare richer chains.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::openhuman::agent_graph::definitions::ProductState;
use crate::openhuman::agent_graph::graph::{CompiledGraph, Node, NodeCtx, NodeOutput, StateGraph};
use crate::openhuman::agent_graph::hitl;
use crate::openhuman::agent_graph::types::{Command, GraphError};

/// A signature a built-in agent's `graph.rs` exposes: `pub fn graph() -> GraphBlueprint`.
pub type GraphBuilder = fn() -> GraphBlueprint;

/// The semantic role of a node — maps to a generic executable body and lets the
/// UI label it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Provider call (advances the iteration counter).
    Dispatch,
    /// Split assistant text from tool calls; routes finalize vs continue.
    Parse,
    /// Policy gate (budget / iteration caps).
    StopCheck,
    /// Tool execution.
    Tools,
    /// Context management (microcompact / autocompact).
    Compact,
    /// Final assistant answer (terminal).
    Finalize,
    /// Human-in-the-loop interrupt.
    Hitl,
    /// Delegate to another agent by id.
    Delegate(String),
    /// A bespoke step with a human-readable description.
    Custom(String),
}

impl NodeKind {
    /// Short label for logs / UI.
    pub fn label(&self) -> String {
        match self {
            NodeKind::Dispatch => "dispatch".into(),
            NodeKind::Parse => "parse".into(),
            NodeKind::StopCheck => "stop_check".into(),
            NodeKind::Tools => "tools".into(),
            NodeKind::Compact => "compact".into(),
            NodeKind::Finalize => "finalize".into(),
            NodeKind::Hitl => "hitl".into(),
            NodeKind::Delegate(a) => format!("delegate:{a}"),
            NodeKind::Custom(d) => format!("custom:{d}"),
        }
    }
}

/// A node in the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpec {
    /// Routing id (unique within the blueprint).
    pub id: String,
    /// Semantic role.
    pub kind: NodeKind,
}

/// An edge in the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeSpec {
    /// Always route `from` → `to`.
    Static { from: String, to: String },
    /// Route `from` by a named condition. `targets[0]` is the true branch,
    /// `targets[1]` the false branch.
    Conditional {
        from: String,
        /// Condition name understood by [`GraphBlueprint::compile`]
        /// (`"iter_capped"`, `"approved"`).
        on: String,
        targets: Vec<String>,
    },
    /// Fan `from` out to all `targets` in parallel.
    Fork { from: String, targets: Vec<String> },
}

/// A complete agent chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphBlueprint {
    /// Owning agent id / chain name.
    pub name: String,
    /// Entry node id.
    pub entry: String,
    /// Finish node ids (terminal).
    pub finish: Vec<String>,
    /// Nodes.
    pub nodes: Vec<NodeSpec>,
    /// Edges.
    pub edges: Vec<EdgeSpec>,
}

impl GraphBlueprint {
    /// Ergonomic builder.
    pub fn builder(name: impl Into<String>) -> BlueprintBuilder {
        BlueprintBuilder {
            bp: GraphBlueprint {
                name: name.into(),
                entry: String::new(),
                finish: Vec::new(),
                nodes: Vec::new(),
                edges: Vec::new(),
            },
        }
    }

    /// Structural validation independent of execution: entry/finish/edge targets
    /// all reference declared nodes; entry + at least one finish set.
    pub fn validate(&self) -> Result<(), String> {
        if self.entry.is_empty() {
            return Err(format!("blueprint '{}' has no entry node", self.name));
        }
        if self.finish.is_empty() {
            return Err(format!("blueprint '{}' has no finish node", self.name));
        }
        let known = |id: &str| self.nodes.iter().any(|n| n.id == id);
        if !known(&self.entry) {
            return Err(format!("entry '{}' is not a declared node", self.entry));
        }
        for f in &self.finish {
            if !known(f) {
                return Err(format!("finish '{f}' is not a declared node"));
            }
        }
        for edge in &self.edges {
            let (from, targets): (&str, Vec<&str>) = match edge {
                EdgeSpec::Static { from, to } => (from, vec![to.as_str()]),
                EdgeSpec::Conditional { from, targets, .. } | EdgeSpec::Fork { from, targets } => {
                    (from, targets.iter().map(|s| s.as_str()).collect())
                }
            };
            if !known(from) {
                return Err(format!("edge from unknown node '{from}'"));
            }
            for t in targets {
                if !known(t) {
                    return Err(format!("edge to unknown node '{t}'"));
                }
            }
        }
        Ok(())
    }

    /// Compile to a runnable [`CompiledGraph`] over [`ProductState`], reusing the
    /// engine's `compile()` validation. Each node kind gets a generic executable
    /// body; condition edges map known names to a state router.
    pub fn compile(&self) -> Result<CompiledGraph<ProductState>, GraphError> {
        self.validate().map_err(GraphError::UnknownNode)?;
        let mut g = StateGraph::<ProductState>::new(self.name.clone());
        for node in &self.nodes {
            g.add_node(node.id.clone(), make_node(&node.kind));
        }
        for edge in &self.edges {
            match edge {
                EdgeSpec::Static { from, to } => {
                    g.add_edge(from.clone(), to.clone());
                }
                EdgeSpec::Fork { from, targets } => {
                    g.add_fork(from.clone(), targets.clone());
                }
                EdgeSpec::Conditional { from, on, targets } => {
                    let on = on.clone();
                    let targets_cl = targets.clone();
                    g.add_conditional_edges(
                        from.clone(),
                        targets.clone(),
                        Box::new(move |s: &ProductState| route(&on, &targets_cl, s)),
                    );
                }
            }
        }
        g.set_entry_point(self.entry.clone());
        for f in &self.finish {
            g.set_finish_point(f.clone());
        }
        g.compile()
    }
}

/// Evaluate a named condition against state, returning the chosen target.
fn route(on: &str, targets: &[String], s: &ProductState) -> String {
    let truthy = match on {
        "iter_capped" => iter_count(s) >= max_iters(s),
        "approved" => s
            .vars
            .get("review_decision")
            .and_then(|v| v.as_str())
            .map(|d| d.eq_ignore_ascii_case("approve"))
            .unwrap_or(false),
        _ => true,
    };
    let idx = if truthy { 0 } else { 1 };
    targets
        .get(idx)
        .or_else(|| targets.first())
        .cloned()
        .unwrap_or_default()
}

fn max_iters(s: &ProductState) -> i64 {
    s.vars
        .get("max_iterations")
        .and_then(|v| v.as_i64())
        .unwrap_or(10)
}

fn iter_count(s: &ProductState) -> i64 {
    s.vars.get("__iter").and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Fluent builder.
pub struct BlueprintBuilder {
    bp: GraphBlueprint,
}

impl BlueprintBuilder {
    /// Add a node.
    pub fn node(mut self, id: &str, kind: NodeKind) -> Self {
        self.bp.nodes.push(NodeSpec {
            id: id.to_string(),
            kind,
        });
        self
    }
    /// Static edge.
    pub fn edge(mut self, from: &str, to: &str) -> Self {
        self.bp.edges.push(EdgeSpec::Static {
            from: from.to_string(),
            to: to.to_string(),
        });
        self
    }
    /// Conditional edge (`targets[0]` true / `targets[1]` false).
    pub fn cond(mut self, from: &str, on: &str, targets: &[&str]) -> Self {
        self.bp.edges.push(EdgeSpec::Conditional {
            from: from.to_string(),
            on: on.to_string(),
            targets: targets.iter().map(|s| s.to_string()).collect(),
        });
        self
    }
    /// Parallel fan-out edge.
    pub fn fork(mut self, from: &str, targets: &[&str]) -> Self {
        self.bp.edges.push(EdgeSpec::Fork {
            from: from.to_string(),
            targets: targets.iter().map(|s| s.to_string()).collect(),
        });
        self
    }
    /// Entry node.
    pub fn entry(mut self, id: &str) -> Self {
        self.bp.entry = id.to_string();
        self
    }
    /// Mark a finish node.
    pub fn finish(mut self, id: &str) -> Self {
        self.bp.finish.push(id.to_string());
        self
    }
    /// Done.
    pub fn build(self) -> GraphBlueprint {
        self.bp
    }
}

/// The standard single-agent tool-calling chain (the canonical turn): the same
/// `dispatch → parse → stop_check → tools → compact → loop / finalize` topology
/// the production turn runs, parameterised by `agent_id`. Most agents reuse it.
pub fn canonical_turn(agent_id: &str) -> GraphBlueprint {
    GraphBlueprint::builder(agent_id)
        .node("dispatch", NodeKind::Dispatch)
        .node("parse", NodeKind::Parse)
        .node("stop_check", NodeKind::StopCheck)
        .node("tools", NodeKind::Tools)
        .node("compact", NodeKind::Compact)
        .node("finalize", NodeKind::Finalize)
        .entry("dispatch")
        .edge("dispatch", "parse")
        .cond("parse", "iter_capped", &["finalize", "stop_check"])
        .cond("stop_check", "iter_capped", &["finalize", "tools"])
        .edge("tools", "compact")
        .edge("compact", "dispatch")
        .finish("finalize")
        .build()
}

/// A single-shot chain: one provider call, then finalize. For agents that
/// classify / summarise / react in one pass (summarizer, trigger_triage,
/// morning_briefing, …) rather than looping on tools.
pub fn single_shot(agent_id: &str) -> GraphBlueprint {
    GraphBlueprint::builder(agent_id)
        .node("dispatch", NodeKind::Dispatch)
        .node("finalize", NodeKind::Finalize)
        .entry("dispatch")
        .edge("dispatch", "finalize")
        .finish("finalize")
        .build()
}

/// An orchestration chain: dispatch → parse → (finalize | delegate to a
/// specialist) → compact → loop. For the top-level orchestrator that routes
/// work to sub-agents instead of running leaf tools itself.
pub fn orchestrator(agent_id: &str) -> GraphBlueprint {
    GraphBlueprint::builder(agent_id)
        .node("dispatch", NodeKind::Dispatch)
        .node("parse", NodeKind::Parse)
        .node("delegate", NodeKind::Delegate("specialist".into()))
        .node("compact", NodeKind::Compact)
        .node("finalize", NodeKind::Finalize)
        .entry("dispatch")
        .edge("dispatch", "parse")
        .cond("parse", "iter_capped", &["finalize", "delegate"])
        .edge("delegate", "compact")
        .edge("compact", "dispatch")
        .finish("finalize")
        .build()
}

/// A plan → execute → review (HITL) → finalize chain for agents that decompose
/// then act under human approval.
pub fn plan_execute_review(agent_id: &str) -> GraphBlueprint {
    GraphBlueprint::builder(agent_id)
        .node("plan", NodeKind::Custom("plan".into()))
        .node("execute", NodeKind::Tools)
        .node("review", NodeKind::Hitl)
        .node("finalize", NodeKind::Finalize)
        .entry("plan")
        .edge("plan", "execute")
        .edge("execute", "review")
        .edge("review", "finalize")
        .finish("finalize")
        .build()
}

// ── Generic executable node bodies (one per NodeKind) ────────────────────────

fn make_node(kind: &NodeKind) -> Arc<dyn Node<ProductState>> {
    match kind {
        NodeKind::Dispatch => Arc::new(DispatchNode),
        NodeKind::Parse => Arc::new(PassNode("parse")),
        NodeKind::StopCheck => Arc::new(PassNode("stop_check")),
        NodeKind::Tools => Arc::new(RecordNode("tools".to_string())),
        NodeKind::Compact => Arc::new(PassNode("compact")),
        NodeKind::Finalize => Arc::new(FinalizeNode),
        NodeKind::Hitl => Arc::new(HitlNode),
        NodeKind::Delegate(agent) => Arc::new(DelegateNode(agent.clone())),
        NodeKind::Custom(desc) => Arc::new(RecordNode(desc.clone())),
    }
}

struct DispatchNode;
#[async_trait]
impl Node<ProductState> for DispatchNode {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        let next = iter_count(&s) + 1;
        s.vars.insert("__iter".to_string(), Value::from(next));
        Ok(NodeOutput::cont(s))
    }
}

/// Follows the declared edge (Parse/StopCheck/Compact route via conditional or
/// static edges, so the node itself just continues).
struct PassNode(&'static str);
#[async_trait]
impl Node<ProductState> for PassNode {
    async fn run(&self, s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        Ok(NodeOutput::cont(s))
    }
}

struct RecordNode(String);
#[async_trait]
impl Node<ProductState> for RecordNode {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        let n = iter_count(&s);
        s.record_step(&self.0, format!("{} step (iteration {n})", self.0));
        Ok(NodeOutput::cont(s))
    }
}

struct DelegateNode(String);
#[async_trait]
impl Node<ProductState> for DelegateNode {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        s.record_step("delegate", format!("delegated to {}", self.0));
        Ok(NodeOutput::cont(s))
    }
}

struct FinalizeNode;
#[async_trait]
impl Node<ProductState> for FinalizeNode {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        let n = iter_count(&s);
        s.set_var("final", format!("completed after {n} iteration(s)"));
        Ok(NodeOutput::end(s))
    }
}

struct HitlNode;
#[async_trait]
impl Node<ProductState> for HitlNode {
    async fn run(&self, mut s: ProductState, _c: &NodeCtx<'_>) -> Result<NodeOutput<ProductState>> {
        if let Some(decision) = s.resume_input.take() {
            s.set_var("review_decision", decision);
            return Ok(NodeOutput::cont(s));
        }
        if s.vars
            .get("auto_approve")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            s.set_var("review_decision", "approve");
            return Ok(NodeOutput::cont(s));
        }
        let mut req = hitl::approval("Approve?", vec![]);
        req.resume_to = Some("review".to_string());
        Ok(NodeOutput {
            state: s,
            command: Command::Interrupt(req),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_turn_validates_and_compiles() {
        let bp = canonical_turn("researcher");
        assert_eq!(bp.name, "researcher");
        bp.validate().expect("structural");
        bp.compile().expect("compiles to a valid graph");
    }

    #[tokio::test]
    async fn canonical_turn_blueprint_runs_to_completion() {
        let bp = canonical_turn("researcher");
        let graph = bp.compile().unwrap();
        let mut init = ProductState::default();
        init.vars
            .insert("max_iterations".to_string(), Value::from(2));
        let out = graph.invoke(init).await.expect("run");
        assert_eq!(
            out.status,
            crate::openhuman::agent_graph::types::GraphRunStatus::Completed
        );
        assert!(out.state.vars.contains_key("final"));
    }

    #[test]
    fn validate_rejects_dangling_edge() {
        let bp = GraphBlueprint::builder("bad")
            .node("a", NodeKind::Dispatch)
            .entry("a")
            .edge("a", "ghost")
            .finish("a")
            .build();
        assert!(bp.validate().is_err());
    }

    #[test]
    fn plan_execute_review_compiles() {
        plan_execute_review("planner").compile().expect("compiles");
    }
}
