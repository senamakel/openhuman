use super::*;
use crate::core::runtime::{ContextOverlay, CoreContext, DomainSet};

fn named(id: &str, when: &str) -> AgentDefinition {
    let mut definition = AgentDefinitionRegistry::builtins_only()
        .get("orchestrator")
        .cloned()
        .expect("built-in orchestrator");
    definition.id = id.to_string();
    definition.when_to_use = when.to_string();
    definition
}

#[test]
fn with_definitions_adds_and_replaces_without_touching_the_source() {
    let base = AgentDefinitionRegistry::builtins_only();
    let before = base.len();
    let own = base.with_definitions([
        named("own-worker", "added"),
        named("orchestrator", "replaced"),
    ]);
    assert_eq!(own.len(), before + 1);
    assert_eq!(own.get("own-worker").unwrap().when_to_use, "added");
    assert_eq!(own.get("orchestrator").unwrap().when_to_use, "replaced");
    assert!(base.get("own-worker").is_none());
    assert_ne!(base.get("orchestrator").unwrap().when_to_use, "replaced");
}

#[tokio::test]
async fn current_follows_the_agent_context_and_falls_back_to_the_process() {
    let own = Arc::new(
        AgentDefinitionRegistry::builtins_only().with_definitions([named("ctx-only", "mine")]),
    );
    let parent = CoreContext::for_test(DomainSet::full(), Some(std::env::temp_dir()));
    let agent = parent.derive_with(
        ContextOverlay::new(
            crate::config::Config::default(),
            DomainSet::kernel(),
            crate::tools::toolpacks::ToolGroups::none(),
        )
        .session_agent("catalogue-agent")
        .definitions(Arc::clone(&own)),
    );

    let seen = CoreContext::scope(agent, async { AgentDefinitionRegistry::current() })
        .await
        .expect("the agent's registry");
    assert!(Arc::ptr_eq(&seen, &own));

    let outside = CoreContext::scope(parent, async { AgentDefinitionRegistry::current() }).await;
    assert!(
        outside.is_none_or(|registry| registry.get("ctx-only").is_none()),
        "a context without its own catalogue resolves through the process"
    );
}

#[test]
fn builtins_only_registry_holds_builtins_only() {
    assert!(AgentDefinitionRegistry::builtins_only().holds_builtins_only());
}

#[test]
fn an_extra_definition_is_not_builtins_only() {
    let extra =
        AgentDefinitionRegistry::builtins_only().with_definitions([named("own-worker", "added")]);
    assert!(!extra.holds_builtins_only());
}

#[test]
fn a_file_sourced_override_is_not_builtins_only() {
    let mut overridden = named("orchestrator", "from a workspace file");
    overridden.source =
        crate::agent::harness::DefinitionSource::File("/ws/agents/orchestrator.toml".into());
    let registry = AgentDefinitionRegistry::builtins_only().with_definitions([overridden]);
    assert!(!registry.holds_builtins_only());
}

#[test]
fn a_builtin_tagged_override_with_other_contents_is_not_builtins_only() {
    let registry = AgentDefinitionRegistry::builtins_only()
        .with_definitions([named("orchestrator", "replaced but still tagged Builtin")]);
    assert!(!registry.holds_builtins_only());
}
