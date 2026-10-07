//! [`AgentSpec::lockdown`](super::AgentSpec::lockdown): deny-by-default for an
//! agent that faces the public.
//!
//! A lockdown agent reaches only the tools its host named — the
//! [`ToolScopeSpec::Named`](super::ToolScopeSpec::Named) belt plus the host's
//! own in-process tools — and nothing it starts can reach more. The pieces:
//!
//! - the named belt becomes the session tool ceiling (`[agent]
//!   tool_ceiling`, see `openhuman_core::agent::tool_ceiling`), so every other
//!   built-in is never registered, every sub-agent is intersected with it and
//!   every workflow, schedule or flow it could start inherits or refuses it;
//! - MCP servers, the operator's user-scope skills and `install_tool` are off;
//! - every tool group the belt does not name is
//!   [`GroupMode::Off`](openhuman_core::tools::toolpacks::GroupMode::Off);
//! - the autonomy policy is enabled, so the access tier is enforced.
//!
//! Applied after the [`config`](super::AgentSpec::config) escape hatch, so the
//! escape hatch cannot undo it.

use openhuman_core::agent::harness::definition::{AgentDefinition, ToolScope};
use openhuman_core::config::Config;
use openhuman_core::tools::toolpacks::{pack, GroupMode, ToolGroups};

use super::AgentError;

/// Lock `config` and `groups` down to `definition`'s named belt.
pub(crate) fn apply(
    config: &mut Config,
    definition: &AgentDefinition,
    groups: ToolGroups,
) -> Result<ToolGroups, AgentError> {
    let ToolScope::Named(belt) = &definition.tools else {
        return Err(AgentError::Invalid(
            "lockdown needs a named tool belt (ToolScopeSpec::Named): a wildcard belt names \
             no ceiling"
                .to_string(),
        ));
    };
    config.agent.tool_ceiling = Some(belt.clone());
    config.mcp_client.enabled = false;
    config.mcp_client.servers.clear();
    config.autonomy.allow_tool_install = false;
    config.autonomy.enabled = true;
    let named = |id: &str| {
        belt.iter().any(|name| name == id)
            || pack(id).is_some_and(|pack| pack.tools.iter().any(|tool| belt.iter().any(|n| n == tool)))
    };
    let groups = ToolGroups::ids()
        .filter(|id| !named(id))
        .fold(groups, |groups, id| groups.with(id, GroupMode::Off));
    log::debug!(
        "[embed][agent] lockdown id={} ceiling={} tools",
        definition.id,
        belt.len()
    );
    Ok(groups)
}

#[cfg(test)]
#[path = "lockdown_tests.rs"]
mod tests;
