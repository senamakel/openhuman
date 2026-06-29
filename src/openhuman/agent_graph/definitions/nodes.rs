//! Reusable nodes for the built-in product graphs.
//!
//! - [`AgentNode`] runs an existing archetype as a one-shot agent
//!   (`Agent::from_config_for_agent` + `run_single`) — this is how a graph
//!   "composes existing archetypes", the standalone-safe analogue of
//!   `run_subagent` (which requires a live parent turn).
//! - [`StubNode`] is a deterministic, LLM-free node for unit/E2E tests.
//! - [`ReviewGate`] is the human-in-the-loop node.
//! - [`FinalizeNode`] assembles the final answer.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::state::ProductState;
use crate::openhuman::agent::Agent;
use crate::openhuman::agent_graph::graph::{Node, NodeCtx, NodeOutput};
use crate::openhuman::agent_graph::hitl;
use crate::openhuman::agent_graph::types::Command;
use crate::openhuman::config::Config;

/// Runs an archetype against a templated prompt and records its output.
pub struct AgentNode {
    /// Archetype id to run (e.g. `planner`, `code_executor`).
    pub agent_id: String,
    /// Var to read the input text from.
    pub input_var: String,
    /// Var to write the output text to (also the recorded step name).
    pub output_var: String,
    /// `{input}` is replaced with the value of `input_var`.
    pub prompt_template: String,
    /// Shared config for constructing the agent.
    pub config: Arc<Config>,
}

#[async_trait]
impl Node<ProductState> for AgentNode {
    async fn run(
        &self,
        mut state: ProductState,
        ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<ProductState>> {
        let input = state.var_str(&self.input_var);
        let prompt = self.prompt_template.replace("{input}", &input);
        tracing::info!(
            run_id = ctx.run_id,
            agent_id = %self.agent_id,
            output_var = %self.output_var,
            "[agent_graph] AgentNode running archetype"
        );
        let mut agent = Agent::from_config_for_agent(&self.config, &self.agent_id)
            .map_err(|e| anyhow::anyhow!("build agent '{}': {e}", self.agent_id))?;
        let output = agent
            .run_single(&prompt)
            .await
            .map_err(|e| anyhow::anyhow!("agent '{}' run failed: {e}", self.agent_id))?;
        state.record_step(&self.output_var, output);
        Ok(NodeOutput::cont(state))
    }
}

/// Deterministic node for tests: writes `template` (with `{input}` substituted
/// from `input_var`) into `output_var`. No LLM.
pub struct StubNode {
    /// Var to read.
    pub input_var: String,
    /// Var to write (and step name).
    pub output_var: String,
    /// Output template; `{input}` substituted.
    pub template: String,
}

#[async_trait]
impl Node<ProductState> for StubNode {
    async fn run(
        &self,
        mut state: ProductState,
        _ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<ProductState>> {
        let input = state.var_str(&self.input_var);
        let out = self.template.replace("{input}", &input);
        state.record_step(&self.output_var, out);
        Ok(NodeOutput::cont(state))
    }
}

/// Human-in-the-loop review node.
///
/// First pass (no resume input, not auto-approved): pauses with an approval
/// interrupt, asking to resume back into this node. On resume it consumes the
/// answer and routes: `approve` → `finalize`, anything else → `execute` (loop
/// back for another attempt). `vars["auto_approve"] == true` skips the pause
/// (used by tests that don't exercise HITL).
pub struct ReviewGate {
    /// Var holding the artifact under review.
    pub review_var: String,
}

#[async_trait]
impl Node<ProductState> for ReviewGate {
    async fn run(
        &self,
        mut state: ProductState,
        ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<ProductState>> {
        let auto = state
            .vars
            .get("auto_approve")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Some(decision) = state.resume_input.take() {
            state.set_var("review_decision", decision.clone());
            tracing::info!(
                run_id = ctx.run_id,
                decision = %decision,
                "[agent_graph] ReviewGate resumed with human decision"
            );
            if decision.eq_ignore_ascii_case("approve") || decision.eq_ignore_ascii_case("yes") {
                return Ok(NodeOutput::goto(state, "finalize"));
            }
            return Ok(NodeOutput::goto(state, "execute"));
        }

        if auto {
            state.set_var("review_decision", "approve");
            return Ok(NodeOutput::goto(state, "finalize"));
        }

        let artifact = state.var_str(&self.review_var);
        let mut req = hitl::approval(
            format!("Approve this result?\n\n{artifact}"),
            vec!["approve".to_string(), "reject".to_string()],
        );
        // Re-run this node on resume so it can route on the decision.
        req.resume_to = Some("review".to_string());
        Ok(NodeOutput {
            state,
            command: Command::Interrupt(req),
        })
    }
}

/// Assembles the final answer from the recorded result and ends the run.
pub struct FinalizeNode {
    /// Var holding the result to finalize.
    pub result_var: String,
}

#[async_trait]
impl Node<ProductState> for FinalizeNode {
    async fn run(
        &self,
        mut state: ProductState,
        _ctx: &NodeCtx<'_>,
    ) -> Result<NodeOutput<ProductState>> {
        let result = state.var_str(&self.result_var);
        let final_text = format!("Final result:\n{result}");
        state.set_var("final", final_text.clone());
        state.record_step("finalize", final_text);
        Ok(NodeOutput::end(state))
    }
}
