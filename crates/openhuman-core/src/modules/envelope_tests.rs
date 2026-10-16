use super::*;

#[test]
fn accepts_only_the_matching_version_and_exclusive_success_or_error_shape() {
    assert_eq!(
        decode(Some("2.5"), Some("2.5"), true, Some(7), None::<&str>),
        Ok(7)
    );
    assert_eq!(
        decode(Some("2.5"), Some("2.5"), false, None, Some("denied")),
        Err(EnvelopeError::Operation("denied"))
    );
    assert_eq!(
        decode(Some("2.4"), Some("2.5"), true, Some(7), None),
        Err(EnvelopeError::Malformed)
    );
    assert_eq!(
        decode(
            Some("2.5"),
            Some("2.5"),
            true,
            Some(7),
            Some("contradictory")
        ),
        Err(EnvelopeError::Malformed)
    );
    assert_eq!(
        decode(Some("2.5"), Some("2.5"), true, None::<u8>, None::<&str>),
        Err(EnvelopeError::Malformed)
    );
    assert_eq!(
        decode(
            Some("2.5"),
            Some("2.5"),
            false,
            Some(7),
            Some("contradictory")
        ),
        Err(EnvelopeError::Malformed)
    );
    assert_eq!(
        decode(Some("2.5"), Some("2.5"), false, None::<u8>, None::<&str>),
        Err(EnvelopeError::Malformed)
    );
}

#[test]
fn accepts_versionless_agent_envelopes_with_the_same_payload_rules() {
    assert_eq!(decode(None, None, true, Some(7), None::<&str>), Ok(7));
    assert_eq!(
        decode(None, None, false, None::<u8>, Some("failed")),
        Err(EnvelopeError::Operation("failed"))
    );
}
