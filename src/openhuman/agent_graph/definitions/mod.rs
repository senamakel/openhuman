//! Built-in graph definitions + the registry the RPC layer enumerates and runs.
//!
//! All definitions share the [`ProductState`] type so the [`runner`] is
//! monomorphic. The registry maps a name → builder; [`list_definitions`] backs
//! `agent_graph_definition_list`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::openhuman::agent_graph::graph::CompiledGraph;
use crate::openhuman::config::Config;

pub mod canonical_turn;
mod nodes;
pub mod product_graphs;
pub mod runner;
pub mod state;

pub use state::ProductState;

/// Metadata describing a registered graph (for `agent_graph_definition_list`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphDefinitionMeta {
    /// Registry name (the `name` param to run it).
    pub name: String,
    /// One-line purpose.
    pub description: String,
    /// Node ids in topological reading order.
    pub nodes: Vec<String>,
    /// Whether the graph contains a human-in-the-loop interrupt node.
    pub has_hitl: bool,
    /// Whether running it requires a live inference backend (real archetypes).
    pub requires_backend: bool,
}

fn strs(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// Every registered graph definition.
pub fn list_definitions() -> Vec<GraphDefinitionMeta> {
    vec![
        GraphDefinitionMeta {
            name: "canonical_turn".to_string(),
            description: "The canonical agent turn as an explicit state graph (dispatch → parse → \
                 stop_check → tools → compact → loop, or finalize)."
                .to_string(),
            nodes: strs(canonical_turn::NODES),
            has_hitl: false,
            requires_backend: false,
        },
        GraphDefinitionMeta {
            name: "plan_execute_review".to_string(),
            description: "Plan → execute → human review → finalize, composing the planner and \
                 code_executor archetypes around a HITL approval gate."
                .to_string(),
            nodes: strs(product_graphs::NODES),
            has_hitl: true,
            requires_backend: true,
        },
        GraphDefinitionMeta {
            name: "demo_review".to_string(),
            description: "Deterministic twin of plan_execute_review (no LLM) for testing routing, \
                 HITL pause/resume, and checkpointing."
                .to_string(),
            nodes: strs(product_graphs::NODES),
            has_hitl: true,
            requires_backend: false,
        },
    ]
}

/// Build a registered graph by name.
pub fn build_definition(
    name: &str,
    config: Arc<Config>,
) -> Result<CompiledGraph<ProductState>, String> {
    let compiled = match name {
        "canonical_turn" => canonical_turn::build(),
        "plan_execute_review" => product_graphs::build_plan_execute_review(config),
        "demo_review" => product_graphs::build_demo_review(),
        other => return Err(format!("unknown graph definition '{other}'")),
    };
    compiled.map_err(|e| format!("compile graph '{name}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_registered_definitions_compile() {
        let config = Arc::new(Config::default());
        for meta in list_definitions() {
            build_definition(&meta.name, config.clone())
                .unwrap_or_else(|e| panic!("definition '{}' failed to compile: {e}", meta.name));
        }
    }

    #[test]
    fn unknown_definition_errors() {
        let config = Arc::new(Config::default());
        assert!(build_definition("ghost", config).is_err());
    }

    #[test]
    fn registry_lists_canonical_and_product_graphs() {
        let names: Vec<String> = list_definitions().into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"canonical_turn".to_string()));
        assert!(names.contains(&"plan_execute_review".to_string()));
        assert!(names.contains(&"demo_review".to_string()));
    }
}
