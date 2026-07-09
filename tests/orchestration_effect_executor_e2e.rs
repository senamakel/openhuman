//! Phase 1 client effect-executor coverage: parsing the hosted brain's
//! `orch:effect:send_dm` frame, the ack frame it returns, the at-least-once
//! callId dedupe, and the device-tool manifest. Lives in an integration crate
//! (links the compiled lib) because the root cfg(test) build is blocked by
//! unrelated stale test modules at this checkout.

use openhuman_core::openhuman::orchestration::effect_executor::{
    device_tool_manifest, effect_result_frame, is_duplicate_call, parse_send_dm,
};
use serde_json::json;

#[test]
fn parses_a_well_formed_send_dm_frame() {
    let frame = json!({
        "cycleId": "cyc:agent-alice:sess-1:3",
        "callId": "cyc:agent-alice:sess-1:3:send_dm:0",
        "counterpartAgentId": "agent-alice",
        "sessionId": "sess-1",
        "body": "on it"
    });
    let effect = parse_send_dm(&frame).expect("parse");
    assert_eq!(effect.call_id, "cyc:agent-alice:sess-1:3:send_dm:0");
    assert_eq!(effect.counterpart_agent_id, "agent-alice");
    assert_eq!(effect.session_id, "sess-1");
    assert_eq!(effect.body, "on it");
}

#[test]
fn rejects_a_frame_missing_required_fields() {
    let frame = json!({ "cycleId": "c", "body": "hi" }); // no callId / counterpartAgentId
    assert!(parse_send_dm(&frame).is_err());
}

#[test]
fn ack_frame_shapes_ok_and_error_cases() {
    assert_eq!(
        effect_result_frame("call-1", true, None),
        json!({ "callId": "call-1", "ok": true, "error": null })
    );
    assert_eq!(
        effect_result_frame("call-2", false, Some("device offline")),
        json!({ "callId": "call-2", "ok": false, "error": "device offline" })
    );
}

#[test]
fn dedupe_reports_first_call_new_and_repeat_duplicate() {
    // Unique id so the process-global dedupe set can't collide with other tests.
    let id = "dedupe-test-unique-call-id-abc123";
    assert!(!is_duplicate_call(id), "first sighting is not a duplicate");
    assert!(is_duplicate_call(id), "second sighting is a duplicate");
}

#[test]
fn manifest_declares_the_device_signal_send_tool() {
    let manifest = device_tool_manifest();
    let tools = manifest["tools"].as_array().expect("tools array");
    assert!(tools.iter().any(|t| t["name"] == "signal_send"));
}
