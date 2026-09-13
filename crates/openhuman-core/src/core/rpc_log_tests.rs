use super::*;
use serde_json::json;

#[test]
fn test_summarize_rpc_result() {
    assert_eq!(
        summarize_rpc_result(&json!({"b": 2, "a": 1})),
        "object(keys=a,b)"
    );
    assert_eq!(summarize_rpc_result(&json!({})), "object(keys=)");
    assert_eq!(summarize_rpc_result(&json!([1, 2, 3])), "array(len=3)");
    assert_eq!(summarize_rpc_result(&json!([])), "array(len=0)");
    assert_eq!(summarize_rpc_result(&json!("hello")), "string(len=5)");
    assert_eq!(summarize_rpc_result(&json!("")), "string(len=0)");
    assert_eq!(summarize_rpc_result(&json!(true)), "bool(true)");
    assert_eq!(summarize_rpc_result(&json!(false)), "bool(false)");
    assert_eq!(summarize_rpc_result(&json!(42)), "number(42)");
    assert_eq!(summarize_rpc_result(&json!(3.14)), "number(3.14)");
    assert_eq!(summarize_rpc_result(&json!(null)), "null");
}
