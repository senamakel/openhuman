use super::*;
use std::collections::HashSet;
use tinytools::{PermissionLevel, Tool, ToolResult};

struct Fixture {
    name: &'static str,
    level: PermissionLevel,
    external: bool,
}

#[async_trait::async_trait]
impl Tool for Fixture {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "posture fixture"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    fn permission_level(&self) -> PermissionLevel {
        self.level
    }
    fn external_effect(&self) -> bool {
        self.external
    }
    async fn execute(&self, _: serde_json::Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("ok"))
    }
}

fn tool(name: &'static str, level: PermissionLevel, external: bool) -> Box<dyn Tool> {
    Box::new(Fixture {
        name,
        level,
        external,
    })
}

fn names(list: &[&str]) -> HashSet<String> {
    list.iter().map(|name| (*name).to_string()).collect()
}

fn session(visible: &[&str], ceiling: &[&str], channel_readonly: bool) -> OpenHumanSessionHost {
    let mut config = crate::config::AgentConfig::default();
    if channel_readonly {
        config
            .channel_permissions
            .insert("internal".into(), "readonly".into());
    }
    let model: std::sync::Arc<dyn tinyinference_llm::model::ChatModel<()>> =
        std::sync::Arc::new(tinyagents_harness::testkit::ScriptedModel::new(Vec::new()));
    OpenHumanSessionHost::builder()
        .chat_model(model)
        .tools(vec![
            tool("file_read", PermissionLevel::ReadOnly, false),
            tool("host_post", PermissionLevel::ReadOnly, true),
            tool("shell", PermissionLevel::Execute, true),
            tool("spawn_subagent", PermissionLevel::None, false),
        ])
        .visible_tool_names(names(visible))
        .subagent_tool_ceiling_names(names(ceiling))
        .config(config)
        .build()
        .expect("session")
}

fn external() -> AgentTurnOrigin {
    AgentTurnOrigin::ExternalChannel {
        channel: "public".into(),
        sender: None,
        reply_target: String::new(),
        message_id: String::new(),
    }
}

#[test]
fn advertised_tools_without_a_route_are_the_whole_posture() {
    let session = session(&["file_read"], &[], false);
    assert_eq!(session.effective_tool_names(None), ["file_read"]);
}

#[test]
fn a_route_reaches_the_registry_bounded_by_the_ceiling() {
    let open = session(&["file_read", "spawn_subagent"], &[], false);
    assert!(open
        .effective_tool_names(None)
        .contains(&"shell".to_string()));
    let bounded = session(
        &["file_read", "spawn_subagent"],
        &["file_read", "spawn_subagent"],
        false,
    );
    assert_eq!(
        bounded.effective_tool_names(None),
        ["file_read", "spawn_subagent"]
    );
}

#[test]
fn untrusted_origins_lose_external_effects_on_read_only_sessions() {
    let session = session(&["file_read", "host_post"], &[], true);
    assert_eq!(
        session.effective_tool_names(Some(&external())),
        ["file_read"]
    );
    assert_eq!(
        session.effective_tool_names(Some(&AgentTurnOrigin::Cli)),
        ["file_read", "host_post"]
    );
}

#[test]
fn reach_tools_include_delegates() {
    assert!(is_reach_tool("delegate_to"));
    assert!(is_reach_tool("run_workflow"));
    assert!(!is_reach_tool("file_read"));
}
