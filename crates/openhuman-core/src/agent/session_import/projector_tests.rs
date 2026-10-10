use super::*;

#[test]
fn journal_usage_omits_unknown_final_call_counts_and_preserves_measurements() {
    let mut row = TranscriptMessage::assistant("done");
    row.turn_usage = Some(tinyagents_session::transcript::TurnUsage {
        provider: "mock".into(),
        model: "model".into(),
        usage: tinyagents_session::transcript::MessageUsage {
            input: 120,
            output: 30,
            ..Default::default()
        },
        ts: String::new(),
        reasoning_content: None,
        tool_calls: Vec::new(),
        iteration: 2,
    });
    let legacy = journal_extra_metadata(&row).unwrap();
    let counts = &legacy[TURN_USAGE_METADATA_KEY]["usage"];
    assert_eq!(counts["input"], 120);
    assert_eq!(counts["output"], 30);
    assert!(counts.get("last_call_input").is_none());
    assert!(counts.get("last_call_output").is_none());
    let usage = &mut row.turn_usage.as_mut().unwrap().usage;
    usage.last_call_input = 70;
    usage.last_call_output = 10;
    let measured = journal_extra_metadata(&row).unwrap();
    assert_eq!(
        measured[TURN_USAGE_METADATA_KEY]["usage"]["last_call_input"],
        70
    );
    assert_eq!(
        measured[TURN_USAGE_METADATA_KEY]["usage"]["last_call_output"],
        10
    );
}
