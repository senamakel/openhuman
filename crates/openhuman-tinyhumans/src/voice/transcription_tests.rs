use super::*;

#[test]
fn credentials_only_reach_their_authorized_endpoint() {
    for endpoint in [
        "https://api.tinyhumans.ai/openai/v1/audio/transcriptions",
        "https://staging-api.tinyhumans.ai",
        "http://127.0.0.2:5005",
        "http://[::1]:5005",
        "http://localhost:5005",
    ] {
        assert!(
            ensure_endpoint(&url::Url::parse(endpoint).unwrap(), true).is_ok(),
            "{endpoint}"
        );
    }
    let https_other = url::Url::parse("https://backend.example.test").unwrap();
    assert!(ensure_endpoint(&https_other, false).is_ok());
    assert!(ensure_endpoint(&https_other, true).is_err());
    for endpoint in [
        "http://backend.example.test",
        "ftp://localhost",
        "http://[2001:db8::1]",
    ] {
        assert!(
            ensure_endpoint(&url::Url::parse(endpoint).unwrap(), false).is_err(),
            "{endpoint}"
        );
    }
}

#[test]
fn malformed_audio_fails_before_native_sdk_dispatch() {
    for (input, expected) in [
        ("", "audio_base64 is required"),
        ("not-base64!", "invalid base64 audio"),
        (" ", "audio_base64 is required"),
    ] {
        let result = transcription_form(TranscriptionRequest {
            audio_base64: input.into(),
            ..TranscriptionRequest::default()
        });
        assert!(result.unwrap_err().contains(expected));
    }
    let request = TranscriptionRequest {
        audio_base64: "x".repeat(MAX_AUDIO_BASE64_LEN + 1),
        ..TranscriptionRequest::default()
    };
    assert!(transcription_form(request).unwrap_err().contains("32MB"));
    assert!(transcription_form(TranscriptionRequest {
        audio_base64: "AQID".into(),
        mime_type: Some("invalid MIME".into()),
        ..TranscriptionRequest::default()
    })
    .is_err());
    assert!(transcription_form(TranscriptionRequest {
        audio_base64: "AQID".into(),
        language: Some("fr".into()),
        ..TranscriptionRequest::default()
    })
    .is_ok());
}

#[test]
fn native_sdk_failures_keep_authentication_classification() {
    let status = || tinyhumans_sdk::Error::Status {
        status: 401,
        body: serde_json::json!({"error": "rejected"}),
    };
    assert!(matches!(
        classify_error(status(), false),
        VoiceError::SessionExpired { .. }
    ));
    assert!(matches!(
        classify_error(status(), true),
        VoiceError::ApiKeyRejected { .. }
    ));
    assert_eq!(
        classify_error(
            tinyhumans_sdk::Error::ResponseTooLarge {
                max_bytes: MAX_RESPONSE_BYTES
            },
            false
        )
        .to_string(),
        "transcription response exceeds 65536 bytes"
    );
}
