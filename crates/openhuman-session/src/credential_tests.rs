use super::*;
use crate::test_support::{EXPIRED_JWT, LIVE_JWT, LIVE_JWT_NO_SUB, LOCAL_TOKEN, OPAQUE_TOKEN};
use serde_json::json;

#[test]
fn classify_recognises_the_local_session_shape() {
    let local = Credential::classify(LOCAL_TOKEN.as_str());
    assert_eq!(local.kind, CredentialKind::Local);
    assert!(local.is_local());
    assert_eq!(local.expires_at, None);

    let session = Credential::classify(LIVE_JWT.as_str());
    assert_eq!(session.kind, CredentialKind::Session);
    assert!(session.expires_at.is_some());

    let opaque = Credential::classify(OPAQUE_TOKEN);
    assert_eq!(opaque.kind, CredentialKind::Session);
    assert_eq!(opaque.expires_at, None);
}

#[test]
fn local_shape_requires_exactly_three_segments_with_local_signature() {
    assert!(is_local_session_token("a.b.local"));
    assert!(is_local_session_token("  a.b.local  "));
    assert!(!is_local_session_token("a.b.local.c"));
    assert!(!is_local_session_token("a.local"));
    assert!(!is_local_session_token("a.b.LOCAL"));
}

#[test]
fn credential_kind_round_trips_its_wire_values() {
    for kind in [
        CredentialKind::Session,
        CredentialKind::ApiKey,
        CredentialKind::Local,
    ] {
        assert_eq!(CredentialKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(
        CredentialKind::parse(" api-key "),
        Some(CredentialKind::ApiKey)
    );
    assert_eq!(CredentialKind::parse("jwt"), None);
    assert_eq!(
        serde_json::to_value(CredentialKind::ApiKey).unwrap(),
        json!("api-key")
    );
}

#[test]
fn constructors_trim_and_keep_kind() {
    let key = Credential::api_key("  sk-1  ");
    assert_eq!(key.secret, "sk-1");
    assert_eq!(key.kind, CredentialKind::ApiKey);
    assert_eq!(key.expires_at, None);
    let local = Credential::local(format!(" {} ", LOCAL_TOKEN.as_str()));
    assert_eq!(local.secret, *LOCAL_TOKEN);
}

#[test]
fn jwt_exp_decoding_and_liveness() {
    let now = chrono::Utc::now();
    assert!(decode_jwt_exp(&LIVE_JWT).unwrap() > now);
    assert!(decode_jwt_exp(&EXPIRED_JWT).unwrap() < now);
    assert_eq!(decode_jwt_exp(OPAQUE_TOKEN), None);
    assert_eq!(decode_jwt_exp(&LOCAL_TOKEN), None);

    assert!(jwt_is_live(&LIVE_JWT, now).is_some());
    assert!(jwt_is_live(&EXPIRED_JWT, now).is_none());
    assert!(jwt_is_live(OPAQUE_TOKEN, now).is_none());
}

#[test]
fn user_id_from_jwt_claims_prefers_sub() {
    assert_eq!(
        user_id_from_jwt_claims(&LIVE_JWT).as_deref(),
        Some("user-123")
    );
    assert_eq!(user_id_from_jwt_claims(&LIVE_JWT_NO_SUB), None);
    assert_eq!(user_id_from_jwt_claims(OPAQUE_TOKEN), None);
}

#[test]
fn user_id_from_profile_payload_walks_data_and_user() {
    assert_eq!(
        user_id_from_profile_payload(&json!({ "id": "a" })).as_deref(),
        Some("a")
    );
    assert_eq!(
        user_id_from_profile_payload(&json!({ "_id": " b " })).as_deref(),
        Some("b")
    );
    assert_eq!(
        user_id_from_profile_payload(&json!({ "user": { "userId": "c" } })).as_deref(),
        Some("c")
    );
    assert_eq!(
        user_id_from_profile_payload(&json!({ "data": { "user": { "id": "d" } } })).as_deref(),
        Some("d")
    );
    assert_eq!(
        user_id_from_profile_payload(&json!({ "data": { "id": "e" } })).as_deref(),
        Some("e")
    );
    assert_eq!(user_id_from_profile_payload(&json!({ "id": "" })), None);
    assert_eq!(user_id_from_profile_payload(&json!("nope")), None);
}
