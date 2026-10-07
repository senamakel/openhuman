use super::*;

fn workflow(
    tools: crate::agent::harness::definition::ToolScope,
    md: &str,
) -> registry::WorkflowDefinition {
    let mut table = toml::map::Map::new();
    table.insert("id".into(), toml::Value::String("fixture".into()));
    table.insert("when_to_use".into(), toml::Value::String("fixture".into()));
    let mut def: registry::WorkflowDefinition =
        toml::Value::Table(table).try_into().expect("workflow");
    def.definition.tools = tools;
    def.definition.system_prompt =
        crate::agent::harness::definition::PromptSource::Inline(md.into());
    def
}

#[test]
fn declared_tools_join_the_named_belt_and_the_frontmatter() {
    let def = workflow(
        crate::agent::harness::definition::ToolScope::Named(vec!["file_read".into()]),
        "---\nname: fixture\ndescription: d\nallowed-tools: [shell, file_read]\n---\nbody\n",
    );
    assert_eq!(declared_workflow_tools(&def), ["file_read", "shell"]);
}

#[test]
fn a_wildcard_workflow_without_frontmatter_declares_nothing() {
    let def = workflow(
        crate::agent::harness::definition::ToolScope::Wildcard,
        "just a body",
    );
    assert!(declared_workflow_tools(&def).is_empty());
}
