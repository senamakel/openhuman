//! `ToolPolicyMiddleware::untrusted_origin_block`: an untrusted remote turn on
//! a read-only session is refused external effects at once, by the origin the
//! run context hands it.

use super::*;
use crate::agent::turn_origin::AgentTurnOrigin;
use crate::tools::agent_policy::{
    TaskProfile, TaskRiskLevel, ToolPolicyAction, ToolPolicyDecision, ToolPolicySession,
};
use tinytools::{PermissionLevel, ToolResult};

struct Posting;

#[async_trait]
impl tinytools::Tool for Posting {
    fn name(&self) -> &str {
        "post_reply"
    }
    fn description(&self) -> &str {
        "posts"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object" })
    }
    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("posted"))
    }
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }
    fn external_effect(&self) -> bool {
        true
    }
}

fn middleware(allowed: PermissionLevel) -> ToolPolicyMiddleware {
    let decision = ToolPolicyDecision {
        tool_name: "post_reply".into(),
        action: ToolPolicyAction::Allow,
        required_permission: Some(PermissionLevel::ReadOnly),
        allowed_permission: allowed,
    };
    let session = ToolPolicySession {
        profile: TaskProfile {
            agent_id: "public".into(),
            channel: "internal".into(),
            entrypoint: "session".into(),
            risk_level: TaskRiskLevel::Low,
            allowed_permission: allowed,
        },
        capabilities: Vec::new(),
        allowed_tool_names: ["post_reply".to_string()].into_iter().collect(),
        blocked_tool_names: Default::default(),
        hidden_tool_names: Default::default(),
        decisions: [("post_reply".to_string(), decision)].into_iter().collect(),
    };
    ToolPolicyMiddleware::new(
        Arc::new(crate::agent::tool_policy::AllowAllToolPolicy),
        session,
        vec![Arc::new(vec![Box::new(Posting) as Box<dyn tinytools::Tool>])],
        "sess".into(),
        "internal".into(),
        "public".into(),
    )
}

fn call() -> TaToolCall {
    TaToolCall {
        id: "call-1".into(),
        name: "post_reply".into(),
        arguments: json!({}),
        invalid: None,
    }
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
fn an_untrusted_read_only_turn_is_refused_an_external_effect() {
    let refused = middleware(PermissionLevel::ReadOnly)
        .untrusted_origin_block(Some(&external()), &call())
        .expect("refused");
    assert!(refused.contains("`post_reply`") && refused.contains("untrusted"));
}

#[test]
fn trusted_origins_and_wider_sessions_fall_through_to_the_gate() {
    let read_only = middleware(PermissionLevel::ReadOnly);
    assert!(read_only
        .untrusted_origin_block(Some(&AgentTurnOrigin::Cli), &call())
        .is_none());
    assert!(read_only.untrusted_origin_block(None, &call()).is_none());
    assert!(middleware(PermissionLevel::Write)
        .untrusted_origin_block(Some(&external()), &call())
        .is_none());
}
