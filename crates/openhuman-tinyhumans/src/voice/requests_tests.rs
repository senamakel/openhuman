use super::*;

#[test]
fn speech_wire_shape_preserves_settings_and_omits_empty_hints() {
    let body = speech_body(SynthesisRequest {
        text: " reply ".into(),
        voice_id: Some(" voice ".into()),
        model_id: Some(" ".into()),
        output_format: Some("mp3".into()),
        voice_settings: Some(json!({"stability": 0.5})),
    })
    .unwrap();
    assert_eq!(
        body,
        json!({"text": "reply", "with_visemes": true, "voice_id": "voice", "output_format": "mp3", "voice_settings": {"stability": 0.5}})
    );
    assert!(speech_body(SynthesisRequest::default()).is_err());
    assert!(!speech_body(SynthesisRequest {
        text: "reply".into(),
        voice_settings: Some(Value::Null),
        ..SynthesisRequest::default()
    })
    .unwrap()
    .as_object()
    .unwrap()
    .contains_key("voice_settings"));
}

#[test]
fn hosted_sessions_accept_wrapped_or_bare_values_and_reject_missing_urls() {
    let signed = json!({"signedUrl": "wss://signed", "agentId": "agent"});
    assert_eq!(
        voice_agent_session(&signed).unwrap(),
        voice_agent_session(&json!({"success": true, "data": signed})).unwrap()
    );
    assert!(
        matches!(voice_agent_session(&signed).unwrap(), RealtimeSession::VoiceAgent { user_token, .. } if user_token.is_empty())
    );
    assert!(voice_agent_session(&json!({"agentId": "agent"})).is_err());
    let ticket = json!({"wsUrl": "wss://relay", "sessionId": "session", "model": "gemini"});
    assert_eq!(
        gemini_session(&ticket).unwrap(),
        gemini_session(&json!({"data": ticket})).unwrap()
    );
    assert!(gemini_session(&json!({"model": "gemini"})).is_err());
}
