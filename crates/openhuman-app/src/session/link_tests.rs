use super::decode_rpc_response;
use serde_json::json;

#[test]
fn decodes_result_and_error_envelopes() {
    let ok = decode_rpc_response(
        200,
        r#"{"jsonrpc":"2.0","id":1,"result":{"isAuthenticated":true}}"#,
    )
    .unwrap();
    assert_eq!(ok, json!({ "isAuthenticated": true }));

    let err = decode_rpc_response(
        200,
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"token is required"}}"#,
    )
    .unwrap_err();
    assert_eq!(err, "token is required");

    let http = decode_rpc_response(401, r#"{"jsonrpc":"2.0","id":1}"#).unwrap_err();
    assert!(http.contains("401"), "{http}");

    let garbage = decode_rpc_response(200, "<html>").unwrap_err();
    assert!(garbage.contains("non-JSON"), "{garbage}");
}
