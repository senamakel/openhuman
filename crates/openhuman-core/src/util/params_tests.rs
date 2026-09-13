use super::*;
use serde_json::json;

fn map(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .cloned()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[test]
fn read_required_returns_value_when_present_and_well_typed() {
    let params = map(&[("name", json!("alice"))]);
    let value: String = read_required(&params, "name").expect("present");
    assert_eq!(value, "alice");
}

#[test]
fn read_required_errors_when_missing() {
    let params = map(&[]);
    let err = read_required::<String>(&params, "name").expect_err("missing");
    assert!(err.contains("missing required param 'name'"), "{err}");
}

#[test]
fn read_required_errors_when_wrong_shape() {
    let params = map(&[("name", json!(42))]);
    let err = read_required::<String>(&params, "name").expect_err("wrong shape");
    assert!(err.contains("invalid 'name'"), "{err}");
}

#[test]
fn read_optional_treats_missing_and_null_as_none() {
    let missing = map(&[]);
    assert_eq!(read_optional::<String>(&missing, "name").unwrap(), None);

    let null = map(&[("name", Value::Null)]);
    assert_eq!(read_optional::<String>(&null, "name").unwrap(), None);
}

#[test]
fn read_optional_returns_some_when_present() {
    let params = map(&[("count", json!(3))]);
    assert_eq!(read_optional::<u32>(&params, "count").unwrap(), Some(3));
}
