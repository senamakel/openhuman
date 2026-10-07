//! A session's tool posture: every tool a turn on it can reach.
//!
//! [`OpenHumanSessionHost::effective_tool_names`] answers "what could this
//! agent ever run under this origin?" for a host's posture tests. It is the
//! provider-facing list a turn advertises, plus every tool reachable through
//! a route that leads past that list — a sub-agent, a workflow run, a pack
//! listing, a deferred search — bounded by what the session registered and by
//! its tool ceiling (`agent::tool_ceiling`). A tool the session's policy
//! refuses under the origin is left out, by the same predicates the turn's
//! tool-policy middleware applies.
//!
//! It over-approximates the nested half on purpose: a route counts as
//! reaching every registered tool, which is a sound upper bound because a
//! nested run is built from this session's registry and ceiling.

use std::collections::BTreeSet;

use super::types::OpenHumanSessionHost;
use crate::agent::turn_origin::AgentTurnOrigin;

/// Tools that lead to other tools rather than acting themselves.
const REACH_TOOLS: &[&str] = &[
    "spawn_subagent",
    "spawn_async_subagent",
    "spawn_parallel_agents",
    "spawn_worker_thread",
    "run_workflow",
    "use_skill",
    "tool_search",
    "cron",
    "cron_add",
    "schedule",
    "run_flow",
    "resume_flow_run",
];

/// Whether `name` starts nested work that can call other tools.
pub(crate) fn is_reach_tool(name: &str) -> bool {
    REACH_TOOLS.contains(&name) || name.starts_with("delegate")
}

impl OpenHumanSessionHost {
    /// Every tool a turn on this session can reach under `origin`, sorted.
    ///
    /// See the module docs for what "reach" covers and why it is an upper
    /// bound for nested runs.
    pub fn effective_tool_names(&self, origin: Option<&AgentTurnOrigin>) -> Vec<String> {
        let admitted = |tool: &dyn tinytools::Tool| {
            let decision = self.tool_policy_session.decision_for(tool.name());
            !decision.blocks_execution()
                && !crate::tools::agent_policy::untrusted::refuses(
                    origin,
                    decision.allowed_permission,
                    tool.external_effect(),
                )
        };
        let registered: Vec<&dyn tinytools::Tool> = self
            .tools
            .iter()
            .chain(self.synthesized_tools.iter())
            .map(|tool| tool.as_ref())
            .collect();
        let advertised: BTreeSet<String> = self
            .visible_tool_specs
            .iter()
            .filter(|spec| {
                registered
                    .iter()
                    .find(|tool| tool.name() == spec.name)
                    .is_none_or(|tool| admitted(*tool))
            })
            .map(|spec| spec.name.clone())
            .collect();
        let mut effective = advertised.clone();
        if advertised.iter().any(|name| is_reach_tool(name)) {
            let ceiling = &self.subagent_tool_ceiling_names;
            effective.extend(
                registered
                    .iter()
                    .filter(|tool| ceiling.is_empty() || ceiling.contains(tool.name()))
                    .filter(|tool| admitted(**tool))
                    .map(|tool| tool.name().to_string()),
            );
        }
        log::debug!(
            "[agent::posture] agent={} origin={} advertised={} effective={}",
            self.agent_definition_name,
            origin.map_or_else(|| "none".to_string(), AgentTurnOrigin::class),
            advertised.len(),
            effective.len()
        );
        effective.into_iter().collect()
    }
}

#[cfg(test)]
#[path = "posture_tests.rs"]
mod tests;
