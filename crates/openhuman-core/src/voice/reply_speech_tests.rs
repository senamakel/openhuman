use super::*;
use tinyvoice_bus::Error;

#[test]
fn tts_unauthorized_flattens_to_session_expiry_not_hard_error() {
    let flat = cloud_voice_provider::product_error(Error::SessionExpired {
        message: "backend rejected session token on POST /openai/v1/audio/speech".into(),
    });
    assert!(flat.starts_with("SESSION_EXPIRED:"));
    assert!(flat.contains("/openai/v1/audio/speech"));
    assert!(crate::core::observability::is_session_expired_message(
        &flat
    ));
}

#[test]
fn tts_non_auth_error_is_not_demoted_to_session_expiry() {
    let flat = cloud_voice_provider::product_error(Error::Transport {
        message: "backend request POST /openai/v1/audio/speech: connect timeout".into(),
    });
    assert!(!flat.contains("SESSION_EXPIRED"));
    assert!(flat.contains("connect timeout"));
    assert!(!crate::core::observability::is_session_expired_message(
        &flat
    ));
}
