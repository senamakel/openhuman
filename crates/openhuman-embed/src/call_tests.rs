use super::*;
use serde_json::json;

#[test]
fn unwraps_a_logged_outcome() {
    let wire = json!({
        "result": { "browser_allow_all": true, "log_prompts": false },
        "logs": ["runtime flags read"]
    });
    assert_eq!(
        unwrap_outcome_envelope(wire),
        json!({ "browser_allow_all": true, "log_prompts": false })
    );
}

#[test]
fn passes_through_an_unlogged_value() {
    let wire = json!({ "browser_allow_all": true, "log_prompts": false });
    assert_eq!(unwrap_outcome_envelope(wire.clone()), wire);
}

#[test]
fn passes_through_non_objects() {
    assert_eq!(unwrap_outcome_envelope(json!(7)), json!(7));
    assert_eq!(unwrap_outcome_envelope(json!(null)), json!(null));
    assert_eq!(unwrap_outcome_envelope(json!([1, 2])), json!([1, 2]));
}

#[test]
fn does_not_unwrap_a_domain_type_that_merely_has_a_result_field() {
    // Three keys — not an envelope.
    let wire = json!({ "result": "ok", "logs": ["a"], "extra": 1 });
    assert_eq!(unwrap_outcome_envelope(wire.clone()), wire);

    // `logs` is not an array of strings — not an envelope.
    let wire = json!({ "result": "ok", "logs": 3 });
    assert_eq!(unwrap_outcome_envelope(wire.clone()), wire);

    let wire = json!({ "result": "ok", "logs": [{ "level": "info" }] });
    assert_eq!(unwrap_outcome_envelope(wire.clone()), wire);

    // Two keys but no `result` — not an envelope.
    let wire = json!({ "value": "ok", "logs": ["a"] });
    assert_eq!(unwrap_outcome_envelope(wire.clone()), wire);
}

#[test]
fn unwraps_an_empty_log_array_envelope() {
    // Defensive: `into_cli_compatible_json` does not currently emit this
    // (empty logs return the bare value), but the shape is unambiguous.
    let wire = json!({ "result": { "a": 1 }, "logs": [] });
    assert_eq!(unwrap_outcome_envelope(wire), json!({ "a": 1 }));
}
