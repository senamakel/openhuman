//! The session tool ceiling at build time (see [`crate::agent::tool_ceiling`]).
//!
//! A ceiling on the session's config removes every registered and synthesised
//! tool outside it, and the names that survive — the ceiling plus the host's
//! own belt — become the ceiling every sub-agent of the session inherits.

use std::collections::HashSet;

use tinytools::Tool;

use crate::agent::harness::definition::NO_TOOLS_SENTINEL;
use crate::agent::tool_ceiling::ToolCeiling;

/// Drop every registered tool outside `config`'s ceiling; returns the ceiling.
pub(super) fn apply(
    config: &crate::config::Config,
    agent_id: &str,
    tools: &mut Vec<Box<dyn Tool>>,
) -> Option<ToolCeiling> {
    let ceiling = ToolCeiling::from_config(&config.agent)?;
    let dropped = ceiling.retain_tools(tools);
    log::info!(
        "[agent::builder] tool ceiling applied agent_id={agent_id} kept={} dropped={dropped}",
        tools.len()
    );
    Some(ceiling)
}

/// Drop synthesised delegation tools outside the ceiling: a `delegate_*` route
/// the host did not name does not exist for the session.
pub(super) fn retain(ceiling: Option<&ToolCeiling>, tools: &mut Vec<Box<dyn Tool>>) {
    if let Some(ceiling) = ceiling {
        ceiling.retain_tools(tools);
    }
}

/// The ceiling a ceiling session's children inherit: exactly what it
/// registered. Never empty, because an empty set means "no ceiling" to the
/// sub-agent runner.
pub(super) fn for_children(
    ceiling: Option<&ToolCeiling>,
    tools: &[Box<dyn Tool>],
    synthesized: &[Box<dyn Tool>],
) -> Option<HashSet<String>> {
    ceiling?;
    Some(
        tools
            .iter()
            .chain(synthesized)
            .map(|tool| tool.name().to_string())
            .chain(std::iter::once(NO_TOOLS_SENTINEL.to_string()))
            .collect(),
    )
}

#[cfg(test)]
#[path = "ceiling_tests.rs"]
mod tests;
