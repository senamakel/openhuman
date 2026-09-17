use super::*;
use serde_json::json;

#[test]
fn new_preserves_value_and_logs() {
    let outcome: RpcOutcome<i64> = RpcOutcome::new(7, vec!["a".into(), "b".into()]);
    assert_eq!(outcome.value, 7);
    assert_eq!(outcome.logs, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn single_log_stores_exactly_one_log() {
    let outcome = RpcOutcome::single_log(json!({"ok": true}), "hello");
    assert_eq!(outcome.logs.len(), 1);
    assert_eq!(outcome.logs[0], "hello");
    assert_eq!(outcome.value, json!({"ok": true}));
}

#[test]
fn single_log_accepts_string_and_str_via_into() {
    let a = RpcOutcome::single_log(json!(1), "static str");
    let b = RpcOutcome::single_log(json!(1), String::from("owned string"));
    assert_eq!(a.logs[0], "static str");
    assert_eq!(b.logs[0], "owned string");
}

#[test]
fn into_cli_compatible_json_no_logs_returns_bare_value() {
    let outcome = RpcOutcome::<serde_json::Value>::new(json!({"x": 1}), vec![]);
    let out = outcome.into_cli_compatible_json().unwrap();
    assert_eq!(out, json!({"x": 1}));
    assert!(out.get("logs").is_none());
}

#[test]
fn into_cli_compatible_json_with_logs_wraps_in_envelope() {
    let outcome = RpcOutcome::single_log(json!(42), "did something");
    let out = outcome.into_cli_compatible_json().unwrap();
    assert_eq!(out["result"], json!(42));
    assert_eq!(out["logs"], json!(["did something"]));
    // And only those two keys exist.
    assert_eq!(out.as_object().unwrap().len(), 2);
}

#[test]
fn into_cli_compatible_json_serializes_typed_value() {
    #[derive(serde::Serialize)]
    struct Payload<'a> {
        name: &'a str,
        count: u32,
    }
    let outcome = RpcOutcome::new(
        Payload {
            name: "atlas",
            count: 3,
        },
        vec![],
    );
    let out = outcome.into_cli_compatible_json().unwrap();
    assert_eq!(out, json!({"name": "atlas", "count": 3}));
}

#[test]
fn into_cli_compatible_json_treats_null_value_as_bare_when_no_logs() {
    let outcome: RpcOutcome<Option<i32>> = RpcOutcome::new(None, vec![]);
    let out = outcome.into_cli_compatible_json().unwrap();
    assert!(out.is_null());
}

#[test]
fn into_cli_compatible_json_preserves_log_order() {
    let outcome = RpcOutcome::new(
        json!({"ok": true}),
        vec!["first".into(), "second".into(), "third".into()],
    );
    let out = outcome.into_cli_compatible_json().unwrap();
    assert_eq!(out["logs"], json!(["first", "second", "third"]));
}

#[test]
fn into_cli_compatible_json_empty_string_logs_still_envelope() {
    // An empty log string is still a log — envelope shape must kick in.
    let outcome = RpcOutcome::new(json!("x"), vec!["".into()]);
    let out = outcome.into_cli_compatible_json().unwrap();
    assert!(out.get("result").is_some());
    assert_eq!(out["logs"], json!([""]));
}
