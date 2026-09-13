use super::*;
use serde_json::json;

#[test]
fn structured_envelope_becomes_domain_error() {
    let raw = StructuredRpcError {
        message: "thread thread-9 not found".to_string(),
        data: Some(json!({ "kind": "ThreadNotFound", "thread_id": "thread-9" })),
        expected_user_state: true,
    }
    .encode();

    let err = CoreError::from_rpc_string("openhuman.threads_get", raw);

    assert!(matches!(err, CoreError::Domain { .. }));
    assert_eq!(err.kind(), Some("ThreadNotFound"));
    assert!(err.is_expected_user_state());
    assert!(!err.is_unavailable());
    assert_eq!(err.method(), "openhuman.threads_get");
}

#[test]
fn unknown_method_becomes_unavailable_not_rpc() {
    // This is the gated-domain path: DomainSet removes the controller, so
    // dispatch reports unknown-method. Hosts must be able to tell this from
    // a real failure or they render an error where they should hide a tab.
    let raw = format!("{UNKNOWN_METHOD_PREFIX}openhuman.flows_list");
    let err = CoreError::from_rpc_string("openhuman.flows_list", raw);

    assert!(err.is_unavailable(), "gated method must map to Unavailable");
    assert!(!err.is_expected_user_state());
    assert_eq!(err.kind(), None);
}

#[test]
fn plain_string_becomes_rpc_error() {
    let err = CoreError::from_rpc_string("openhuman.config_get_config", "disk on fire".into());

    assert!(matches!(err, CoreError::Rpc { .. }));
    assert!(!err.is_unavailable());
    assert!(!err.is_expected_user_state());
}

#[test]
fn structured_envelope_wins_over_unknown_method_prefix() {
    // A domain may legitimately produce a message that starts with the
    // unknown-method prefix. The envelope is authoritative.
    let raw = StructuredRpcError {
        message: format!("{UNKNOWN_METHOD_PREFIX}some.thing"),
        data: Some(json!({ "kind": "BadRequest" })),
        expected_user_state: false,
    }
    .encode();

    let err = CoreError::from_rpc_string("openhuman.x_y", raw);

    assert!(matches!(err, CoreError::Domain { .. }));
    assert!(!err.is_unavailable());
    assert_eq!(err.kind(), Some("BadRequest"));
}

#[test]
fn expected_user_state_defaults_false_when_absent_from_envelope() {
    let raw = StructuredRpcError {
        message: "nope".to_string(),
        data: None,
        expected_user_state: false,
    }
    .encode();

    let err = CoreError::from_rpc_string("openhuman.a_b", raw);
    assert!(!err.is_expected_user_state());
}
