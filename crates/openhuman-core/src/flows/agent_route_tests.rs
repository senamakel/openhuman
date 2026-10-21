use super::*;

use serde_json::json;

fn custom_registry_entry(enabled: bool) -> crate::agent_registry::AgentRegistryEntry {
    use crate::agent_registry::types::{AgentRegistrySource, AgentSubagentPolicy};
    crate::agent_registry::AgentRegistryEntry {
        id: "finance_analyst".to_string(),
        name: "Finance Analyst".to_string(),
        description: "Reviews spend and drafts finance summaries.".to_string(),
        source: AgentRegistrySource::Custom,
        enabled,
        model: Some("hint:reasoning".to_string()),
        system_prompt: Some("You are a meticulous finance analyst.".to_string()),
        tool_allowlist: vec!["memory_search".to_string()],
        tool_denylist: Vec::new(),
        tool_rules: None,
        subagents: AgentSubagentPolicy::default(),
        tags: Vec::new(),
        metadata: json!(null),
    }
}

#[test]
fn harness_model_default_override_normalises_tiers_to_hint_roles() {
    assert_eq!(
        harness_model_default_override("reasoning-v1"),
        "hint:reasoning"
    );
    assert_eq!(harness_model_default_override("chat-v1"), "hint:chat");
    assert_eq!(
        harness_model_default_override("hint:reasoning"),
        "hint:reasoning"
    );
}

#[test]
fn harness_model_default_override_forwards_raw_models_verbatim() {
    assert_eq!(
        harness_model_default_override("claude-opus-4"),
        "claude-opus-4"
    );
    assert_eq!(
        harness_model_default_override("openai:gpt-4o"),
        "openai:gpt-4o"
    );
    assert_eq!(harness_model_default_override("   "), "hint:chat");
}

#[test]
fn route_for_agent_ref_selects_harness_for_definitions_else_fallback() {
    let _ = crate::agent_registry::host_definition::AgentDefinitionRegistry::init_global_builtins();
    assert_eq!(route_for_agent_ref("workflow_builder"), AgentRoute::Harness);
    assert_eq!(route_for_agent_ref("planner"), AgentRoute::Harness);
    assert_eq!(
        route_for_agent_ref("totally_unknown_custom_agent_xyz"),
        AgentRoute::RegistryFallback
    );
}

#[test]
fn custom_agent_route_requires_an_enabled_registry_entry() {
    assert_eq!(
        route_custom_entry_lookup(Some(&custom_registry_entry(true))),
        AgentRoute::Harness
    );
    assert_eq!(
        route_custom_entry_lookup(Some(&custom_registry_entry(false))),
        AgentRoute::RegistryFallback
    );
    assert_eq!(
        route_custom_entry_lookup(None),
        AgentRoute::RegistryFallback
    );
}
