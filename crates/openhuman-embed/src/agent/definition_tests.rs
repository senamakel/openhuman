use super::*;

#[test]
fn default_is_the_orchestrator_under_the_agents_id() {
    let def = AgentDefinitionSpec::new()
        .into_core("alpha")
        .expect("definition");
    assert_eq!(def.id, "alpha");
    assert_eq!(def.display_name(), "alpha");
    assert!(matches!(def.system_prompt, PromptSource::Dynamic(_)));
    assert!(matches!(def.tools, ToolScope::Wildcard));
    assert_eq!(def.sandbox_mode, SandboxMode::None);
}

#[test]
fn setters_override_one_aspect_each() {
    let def = AgentDefinitionSpec::new()
        .system_prompt("Be terse.")
        .tools(ToolScopeSpec::Named(vec!["read_file".into()]))
        .disallow_tools(["shell"])
        .sandbox(SandboxModeSpec::Sandboxed)
        .max_iterations(3)
        .temperature(0.1)
        .display_name("Alpha")
        .into_core("alpha")
        .expect("definition");
    assert!(matches!(def.system_prompt, PromptSource::Inline(ref p) if p == "Be terse."));
    assert!(
        matches!(def.tools, ToolScope::Named(ref names) if names == &["read_file".to_string()])
    );
    assert!(def.disallowed_tools.iter().any(|t| t == "shell"));
    assert_eq!(def.sandbox_mode, SandboxMode::Sandboxed);
    assert_eq!(def.max_iterations, 3);
    assert_eq!(def.temperature, 0.1);
    assert_eq!(def.display_name(), "Alpha");
}
