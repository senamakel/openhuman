use super::*;

fn external() -> AgentTurnOrigin {
    AgentTurnOrigin::ExternalChannel {
        channel: "public".into(),
        sender: None,
        reply_target: String::new(),
        message_id: String::new(),
    }
}

#[test]
fn refuses_external_effects_on_untrusted_read_only_turns() {
    assert!(refuses(Some(&external()), PermissionLevel::ReadOnly, true));
    assert!(refuses(Some(&external()), PermissionLevel::None, true));
}

#[test]
fn admits_reads_trusted_origins_and_wider_sessions() {
    assert!(!refuses(
        Some(&external()),
        PermissionLevel::ReadOnly,
        false
    ));
    assert!(!refuses(Some(&external()), PermissionLevel::Write, true));
    assert!(!refuses(
        Some(&AgentTurnOrigin::Cli),
        PermissionLevel::ReadOnly,
        true
    ));
    assert!(!refuses(None, PermissionLevel::ReadOnly, true));
}

#[test]
fn refusal_names_the_tool() {
    let text = refusal("host_post");
    assert!(text.contains("`host_post`") && text.contains("untrusted"));
}
