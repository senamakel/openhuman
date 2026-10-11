use super::*;
use tinyagents_runtime::ResumeMode;

fn meta() -> tinyagents_session::transcript::TranscriptMeta {
    tinyagents_session::transcript::TranscriptMeta {
        session_id: None,
        parent_session_id: None,
        agent_name: "adapter-test".into(),
        agent_id: Some("adapter-test".into()),
        agent_type: Some("root".into()),
        dispatcher: "test".into(),
        provider: None,
        model: None,
        created: chrono::Utc::now().to_rfc3339(),
        updated: chrono::Utc::now().to_rfc3339(),
        turn_count: 0,
        prefix_message_count: None,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        charged_amount_usd: 0.0,
        thread_id: None,
        task_id: None,
    }
}

#[test]
fn codec_still_rejects_an_orphan_result_after_a_user_message() {
    let transcript = tinyagents_session::transcript::SessionTranscript {
        meta: meta(),
        messages: [
            serde_json::json!({"role":"system","content":"frozen"}),
            serde_json::json!({"role":"tool","id":"legacy-head","content":"trimmed"}),
            serde_json::json!({"role":"user","content":"continue"}),
            serde_json::json!({"role":"tool","id":"later-orphan","content":"invalid"}),
        ]
        .into_iter()
        .map(|row| serde_json::from_value(row).unwrap())
        .collect(),
        tools: None,
    };
    let error = OpenHumanTranscriptCodec
        .decode_history(&transcript)
        .unwrap_err();
    assert!(error.to_string().contains("later-orphan"), "{error}");
    assert_eq!(transcript.messages[1].id.as_deref(), Some("legacy-head"));
}

#[test]
fn codec_retains_authoritative_row_metadata_after_legacy_head_adoption() {
    let transcript = tinyagents_session::transcript::SessionTranscript {
        meta: meta(),
        messages: [
            serde_json::json!({"role":"system","content":"frozen"}),
            serde_json::json!({"role":"tool","id":"legacy-head","content":"trimmed"}),
            serde_json::json!({"role":"user","content":"continue","request_id":"original",
                "extra_metadata":{"retained":"product-value"}}),
        ]
        .into_iter()
        .map(|row| serde_json::from_value(row).unwrap())
        .collect(),
        tools: None,
    };
    let previous = OpenHumanTranscriptCodec
        .decode_history(&transcript)
        .unwrap();
    let mut next = previous.clone();
    next.push(Message::assistant("done"));
    let rows = OpenHumanTranscriptCodec
        .reconcile(
            &transcript.messages,
            &previous,
            &next,
            &TranscriptTurnOptions {
                request_id: Some("new-turn".into()),
                thread_id: None,
                stream: false,
                resume: ResumeMode::Never,
                context: OpenHumanRunContext::new(),
            },
        )
        .unwrap();
    assert_eq!(rows[1].role, "user");
    assert_eq!(rows[1].request_id.as_deref(), Some("original"));
    assert_eq!(
        rows[1].extra_metadata,
        Some(serde_json::json!({"retained":"product-value"}))
    );
    assert_eq!(rows[2].request_id.as_deref(), Some("new-turn"));
    assert_eq!(transcript.messages[1].id.as_deref(), Some("legacy-head"));
}
