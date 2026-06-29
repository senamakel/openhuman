//! Shared state for the built-in product graphs.
//!
//! A single concrete state type lets the runner be monomorphic — it can build,
//! run, persist and resume any registered product graph without generics
//! leaking into the RPC layer.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::openhuman::agent_graph::graph::GraphState;
use crate::openhuman::agent_graph::hitl::ApplyResume;

/// One recorded step's output — the "intermediate results that survive
/// transitions" from issue #4249.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StepRecord {
    /// Node that produced it.
    pub node: String,
    /// The step's text output.
    pub output: String,
}

/// Working state threaded through a product graph.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProductState {
    /// Free-form working variables (inputs + per-node outputs).
    #[serde(default)]
    pub vars: Map<String, Value>,
    /// Ordered intermediate results.
    #[serde(default)]
    pub steps: Vec<StepRecord>,
    /// Human input supplied on resume (consumed by the node that re-runs).
    #[serde(default)]
    pub resume_input: Option<String>,
}

impl ProductState {
    /// Seed state from an initial input object (the run's `input` param).
    pub fn from_input(input: Value) -> Self {
        let vars = match input {
            Value::Object(m) => m,
            Value::Null => Map::new(),
            other => {
                let mut m = Map::new();
                m.insert("input".to_string(), other);
                m
            }
        };
        Self {
            vars,
            ..Default::default()
        }
    }

    /// Read a string var, empty when absent.
    pub fn var_str(&self, key: &str) -> String {
        self.vars
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Set a string var.
    pub fn set_var(&mut self, key: &str, value: impl Into<String>) {
        self.vars
            .insert(key.to_string(), Value::String(value.into()));
    }

    /// Record a step's output and store it under `vars[node]`.
    pub fn record_step(&mut self, node: &str, output: impl Into<String>) {
        let output = output.into();
        self.set_var(node, output.clone());
        self.steps.push(StepRecord {
            node: node.to_string(),
            output,
        });
    }
}

impl GraphState for ProductState {
    fn merge(&mut self, other: Self) -> Result<()> {
        // Vars: last-writer-wins per key (other overrides). Steps: append
        // (dedup exact duplicates from a shared pre-fork prefix). resume_input:
        // keep whichever is set.
        for (k, v) in other.vars {
            self.vars.insert(k, v);
        }
        for step in other.steps {
            if !self.steps.contains(&step) {
                self.steps.push(step);
            }
        }
        if self.resume_input.is_none() {
            self.resume_input = other.resume_input;
        }
        Ok(())
    }
}

impl ApplyResume for ProductState {
    fn apply_resume(&mut self, input: &str) {
        self.resume_input = Some(input.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_input_object_seeds_vars() {
        let s = ProductState::from_input(json!({"task": "build it"}));
        assert_eq!(s.var_str("task"), "build it");
    }

    #[test]
    fn from_input_scalar_wraps_under_input() {
        let s = ProductState::from_input(json!("hello"));
        assert_eq!(s.var_str("input"), "hello");
    }

    #[test]
    fn record_step_sets_var_and_appends() {
        let mut s = ProductState::default();
        s.record_step("plan", "a plan");
        assert_eq!(s.var_str("plan"), "a plan");
        assert_eq!(s.steps.len(), 1);
    }

    #[test]
    fn merge_last_writer_and_dedup_steps() {
        let mut a = ProductState::default();
        a.record_step("plan", "x");
        a.set_var("k", "1");
        let mut b = ProductState::default();
        b.record_step("plan", "x"); // duplicate, deduped
        b.record_step("exec", "y");
        b.set_var("k", "2"); // overrides
        a.merge(b).unwrap();
        assert_eq!(a.var_str("k"), "2");
        assert_eq!(a.steps.len(), 2);
    }

    #[test]
    fn apply_resume_sets_input() {
        let mut s = ProductState::default();
        s.apply_resume("approve");
        assert_eq!(s.resume_input.as_deref(), Some("approve"));
    }
}
