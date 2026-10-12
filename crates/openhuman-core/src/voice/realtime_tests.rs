use super::*;

#[test]
fn result_serializes_snake_case_for_the_wire() {
    let json = serde_json::to_value(VoiceAgentSignedUrl {
        signed_url: "wss://z".into(),
        agent_id: "a4".into(),
        user_token: "tok".into(),
    })
    .unwrap();
    assert_eq!(json.get("signed_url").unwrap(), "wss://z");
    assert_eq!(json.get("agent_id").unwrap(), "a4");
    assert_eq!(json.get("user_token").unwrap(), "tok");
}
