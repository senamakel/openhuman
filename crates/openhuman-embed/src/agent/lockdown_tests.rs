use super::*;

fn definition(tools: ToolScope) -> AgentDefinition {
    let mut definition = crate::AgentDefinitionSpec::new()
        .into_core("locked")
        .expect("definition");
    definition.tools = tools;
    definition
}

#[test]
fn a_named_belt_becomes_the_ceiling_and_closes_the_rest() {
    let mut config = Config::default();
    config.mcp_client.enabled = true;
    config.autonomy.allow_tool_install = true;
    let first_pack = ToolGroups::ids().next().expect("a compiled-in pack");
    let member = pack(first_pack)
        .and_then(|pack| pack.tools.first())
        .expect("a pack member");
    let belt = vec!["file_read".to_string(), (*member).to_string()];
    let groups = apply(
        &mut config,
        &definition(ToolScope::Named(belt.clone())),
        ToolGroups::advertised(),
    )
    .expect("lockdown");
    assert_eq!(config.agent.tool_ceiling, Some(belt));
    assert!(!config.mcp_client.enabled);
    assert!(config.mcp_client.servers.is_empty());
    assert!(!config.autonomy.allow_tool_install);
    assert!(config.autonomy.enabled);
    assert_eq!(groups.mode(first_pack), GroupMode::Advertised);
    for id in ToolGroups::ids().filter(|id| *id != first_pack) {
        let named = pack(id).is_some_and(|pack| pack.tools.contains(member));
        if !named {
            assert_eq!(groups.mode(id), GroupMode::Off, "{id} left open");
        }
    }
}

#[test]
fn a_wildcard_belt_is_refused() {
    let mut config = Config::default();
    let err = apply(
        &mut config,
        &definition(ToolScope::Wildcard),
        ToolGroups::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("named tool belt"), "{err}");
    assert!(config.agent.tool_ceiling.is_none());
}
