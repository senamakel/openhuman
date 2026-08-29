//! Wiring the pack tools into an agent's registry and visible set.

use std::collections::HashSet;
use std::sync::{Arc, Weak};

use super::registry;
use super::tools::{LoadSkillTool, PackRegistryHandle, UseSkillTool, LOAD_SKILL, USE_SKILL};
use crate::openhuman::tools::traits::Tool;

/// Append `load_skill` + `use_skill` to a freshly built registry.
///
/// They start unbound; [`bind_pack_registry`] gives them their view of the
/// registry once it is behind an `Arc`.
pub fn append_pack_tools(tools: &mut Vec<Box<dyn Tool>>) {
    let handle = PackRegistryHandle::default();
    tools.push(Box::new(LoadSkillTool::new(handle.clone())));
    tools.push(Box::new(UseSkillTool::new(handle)));
}

/// Point the pack tools at the registry they live in.
///
/// The handle holds a [`Weak`], so the pack tools referencing the very vector
/// that owns them does not leak. Call this after **every** rebinding of the
/// agent's tool `Arc`; a stale handle degrades to "skill unavailable" rather
/// than dispatching to the wrong registry.
pub fn bind_pack_registry(tools: &Arc<Vec<Box<dyn Tool>>>) {
    let weak: Weak<Vec<Box<dyn Tool>>> = Arc::downgrade(tools);
    let mut bound = 0usize;
    for tool in tools.iter() {
        let name = tool.name();
        if name != LOAD_SKILL && name != USE_SKILL {
            continue;
        }
        if let Some(handle) = crate::openhuman::tools::traits::pack_registry_handle(tool.as_ref()) {
            handle.bind(weak.clone());
            bound += 1;
        }
    }
    tracing::debug!(bound, "[toolpacks] bound pack tools to live registry");
}

/// Remove packed tool names from an agent's advertised set.
///
/// This is the whole compression: the tools stay registered and executable, but
/// their schemas never reach the provider. An agent that declared none of the
/// packed tools is unaffected, and `load_skill` / `use_skill` are only added
/// when the agent actually lost something to a pack — otherwise every narrow
/// sub-agent would grow two tools that can only report an empty skill.
///
/// `agent_id` selects which packs apply: a pack is skipped for the specialist
/// that owns its family (see [`super::types::ToolPack::owners`]), because
/// withholding a belt from the agent that exists to run it only buys a
/// `load_skill` round trip per turn.
///
/// A caller with an *empty* `visible` set means "everything is visible"
/// (the harness's historical sentinel), so there is nothing to subtract from
/// and the set is left alone.
pub fn strip_packed_from_visible(visible: &mut HashSet<String>, agent_id: &str) {
    if visible.is_empty() {
        return;
    }
    // Groups an embedder marked `Advertised` keep their schemas on the wire;
    // `Off` groups were never registered, so nothing of theirs can be in
    // `visible` to subtract. Only `Withheld` — the default for every group —
    // is actually withheld here.
    let groups = super::groups::current();
    let packed: Vec<String> = registry::packed_tool_names_for_agent(agent_id)
        .into_iter()
        .filter(|name| groups.mode_for_tool(name) == super::groups::GroupMode::Withheld)
        .filter(|name| visible.contains(*name))
        .map(str::to_string)
        .collect();
    if packed.is_empty() {
        return;
    }
    for name in &packed {
        visible.remove(name);
    }
    visible.insert(LOAD_SKILL.to_string());
    visible.insert(USE_SKILL.to_string());
    tracing::info!(
        agent = %agent_id,
        hidden = packed.len(),
        "[toolpacks] withheld packed tool schemas; load_skill/use_skill advertised instead"
    );
}
