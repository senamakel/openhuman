use super::*;

fn jwt_with_payload(payload_json: &str) -> String {
    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json);
    format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
}

// The SDK owns the parsing rules and tests them directly. What matters here
// is that the `chrono` conversion this crate depends on stays correct.
#[test]
fn decode_jwt_exp_reads_integer_exp() {
    let token = jwt_with_payload(r#"{"sub":"u1","exp":1700000000}"#);
    assert_eq!(
        decode_jwt_exp(&token),
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
    );
}

#[test]
fn decode_jwt_exp_reads_float_exp() {
    let token = jwt_with_payload(r#"{"exp":1700000000.0}"#);
    assert_eq!(
        decode_jwt_exp(&token),
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
    );
}

#[test]
fn decode_jwt_exp_none_when_exp_absent() {
    let token = jwt_with_payload(r#"{"sub":"u1"}"#);
    assert_eq!(decode_jwt_exp(&token), None);
}

#[test]
fn decode_jwt_exp_none_for_non_jwt_or_garbage() {
    assert_eq!(decode_jwt_exp("not-a-jwt"), None);
    assert_eq!(decode_jwt_exp(""), None);
    assert_eq!(decode_jwt_exp("a.b"), None);
    // Local offline session sentinel (not a JWT) must not panic.
    assert_eq!(decode_jwt_exp("local-session-xyz"), None);
}
