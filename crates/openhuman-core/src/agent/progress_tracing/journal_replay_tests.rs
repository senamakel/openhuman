use super::*;
use tinyagents_harness::terminal::TerminalReason;

#[test]
fn user_message_text_reads_the_last_user_message_in_both_shapes() {
    let role_tagged = serde_json::json!([
        {"role": "system", "content": "You are OpenHuman."},
        {"role": "user", "content": "first question"},
        {"role": "assistant", "content": "answer"},
        {"role": "user", "content": "follow-up"}
    ]);
    assert_eq!(user_message_text(&role_tagged), "follow-up");

    let crate_tagged = serde_json::json!([
        {"system": {"content": [{"text": "sys"}]}},
        {"user": {"content": [{"text": "build me a workflow"}, {"image": {}}]}}
    ]);
    assert_eq!(user_message_text(&crate_tagged), "build me a workflow");
}

#[test]
fn user_message_text_falls_back_without_a_user_message() {
    assert_eq!(user_message_text(&serde_json::json!("plain")), "plain");
    assert_eq!(
        user_message_text(&serde_json::json!({"content": "obj"})),
        "obj"
    );
}

#[test]
fn failure_outcome_maps_terminal_class() {
    let cancelled = TerminalOutcome::new(TerminalReason::Cancelled, "user stopped");
    assert_eq!(
        turn_outcome_for_failure("ignored", Some(&cancelled)),
        TurnOutcome::Cancelled {
            reason: Some("user stopped".into())
        }
    );
    let timeout = TerminalOutcome::new(TerminalReason::Timeout, "deadline");
    assert_eq!(
        turn_outcome_for_failure("ignored", Some(&timeout)),
        TurnOutcome::TimedOut {
            message: "deadline".into()
        }
    );
    assert_eq!(
        turn_outcome_for_failure("provider down", None),
        TurnOutcome::Failed {
            message: "provider down".into()
        }
    );
}
