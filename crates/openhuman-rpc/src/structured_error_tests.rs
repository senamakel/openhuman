use super::*;
use serde_json::json;

#[test]
fn encode_then_decode_round_trips() {
    let original = StructuredRpcError {
        message: "thread thread-123 not found".to_string(),
        data: Some(json!({ "kind": "ThreadNotFound", "thread_id": "thread-123" })),
        expected_user_state: true,
    };
    let encoded = original.encode();
    assert!(
        encoded.starts_with(STRUCTURED_RPC_ERROR_SENTINEL),
        "encoded string must carry the sentinel prefix"
    );
    let decoded = StructuredRpcError::decode(&encoded).expect("decoded");
    assert_eq!(decoded, original);
}

#[test]
fn decode_returns_none_for_plain_strings() {
    assert!(StructuredRpcError::decode("plain error").is_none());
    assert!(StructuredRpcError::decode("").is_none());
    assert!(StructuredRpcError::decode("__OPENHUMAN_STRUCTURED_RPC_ERROR_V1__").is_none());
}

#[test]
fn decode_returns_none_for_corrupt_envelope() {
    let bad = format!("{STRUCTURED_RPC_ERROR_SENTINEL}not-json");
    assert!(StructuredRpcError::decode(&bad).is_none());
}

#[test]
fn expected_user_state_defaults_to_false_when_absent() {
    let raw = format!("{STRUCTURED_RPC_ERROR_SENTINEL}{{\"message\":\"x\",\"data\":null}}");
    let decoded = StructuredRpcError::decode(&raw).expect("decoded");
    assert!(!decoded.expected_user_state);
}
