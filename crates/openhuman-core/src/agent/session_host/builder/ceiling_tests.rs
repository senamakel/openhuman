use super::*;

struct Named(&'static str);

#[async_trait::async_trait]
impl Tool for Named {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "fixture"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(&self, _: serde_json::Value) -> anyhow::Result<tinytools::ToolResult> {
        Ok(tinytools::ToolResult::success("ok"))
    }
}

fn belt(names: &[&'static str]) -> Vec<Box<dyn Tool>> {
    names
        .iter()
        .map(|name| Box::new(Named(name)) as Box<dyn Tool>)
        .collect()
}

#[test]
fn no_ceiling_changes_nothing() {
    let config = crate::config::Config::default();
    let mut tools = belt(&["shell", "file_read"]);
    assert!(apply(&config, "agent", &mut tools).is_none());
    assert_eq!(tools.len(), 2);
    retain(None, &mut tools);
    assert_eq!(tools.len(), 2);
    assert!(for_children(None, &tools, &[]).is_none());
}

#[test]
fn a_ceiling_bounds_the_registry_delegates_and_children() {
    let mut config = crate::config::Config::default();
    config.agent.tool_ceiling = Some(vec!["file_read".into(), "delegate_to".into()]);
    let mut tools = belt(&["shell", "file_read", "host_tool"]);
    let ceiling = apply(&config, "agent", &mut tools).expect("ceiling");
    assert_eq!(tools.len(), 1);
    let mut delegates = belt(&["delegate_to", "delegate_research"]);
    retain(Some(&ceiling), &mut delegates);
    assert_eq!(delegates.len(), 1);
    // Host tools are spliced in after the ceiling, and children inherit them.
    tools.extend(belt(&["host_tool"]));
    let children = for_children(Some(&ceiling), &tools, &delegates).expect("children");
    for name in ["file_read", "host_tool", "delegate_to", NO_TOOLS_SENTINEL] {
        assert!(children.contains(name), "{name} missing: {children:?}");
    }
    assert!(!children.contains("shell"));
}
