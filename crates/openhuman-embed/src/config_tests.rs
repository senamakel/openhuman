use super::*;
use serde_json::json;

#[test]
fn runtime_flags_decodes_the_core_wire_shape() {
    // Exactly what `RuntimeFlagsOut` serializes to. If the core struct
    // gains or renames a field, this fails and the facade type gets
    // updated deliberately rather than drifting.
    let wire = json!({ "browser_allow_all": true, "log_prompts": false });
    let flags: RuntimeFlags = serde_json::from_value(wire).expect("decodes");
    assert!(flags.browser_allow_all);
    assert!(!flags.log_prompts);
}

#[test]
fn runtime_flags_round_trips() {
    let original = RuntimeFlags {
        browser_allow_all: false,
        log_prompts: true,
    };
    let encoded = serde_json::to_value(&original).expect("encodes");
    let decoded: RuntimeFlags = serde_json::from_value(encoded).expect("decodes");
    assert_eq!(decoded, original);
}

#[test]
fn runtime_flags_rejects_a_missing_field() {
    // No `#[serde(default)]`: a shape change must fail loudly at the
    // facade boundary rather than silently defaulting to `false`, which
    // would read as "the flag is off" instead of "we lost the field".
    let wire = json!({ "browser_allow_all": true });
    assert!(serde_json::from_value::<RuntimeFlags>(wire).is_err());
}
