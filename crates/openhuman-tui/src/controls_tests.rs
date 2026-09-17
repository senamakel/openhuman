use super::*;

#[test]
fn rpc_payload_unwraps_runtime_and_api_envelopes() {
    let value = json!({"result": {"data": {"mode": "standard"}}, "logs": []});
    assert_eq!(rpc_payload(&value), &json!({"mode": "standard"}));
    assert_eq!(string_at(rpc_payload(&value), &["mode"]), "standard");
}

#[test]
fn string_at_never_coerces_non_string_or_missing_values() {
    let value = json!({"enabled": true});
    assert_eq!(string_at(&value, &["enabled"]), "");
    assert_eq!(string_at(&value, &["missing"]), "");
}

#[test]
fn account_detail_uses_canonical_backend_user_fields() {
    let user = json!({
        "firstName": "Ada",
        "lastName": "Lovelace",
        "email": "ada@example.test",
        "username": "ada"
    });
    assert_eq!(account_detail(&user), "Ada Lovelace · ada@example.test");
}

#[test]
fn curated_config_fields_map_to_safe_specific_updates() {
    let cases = [
        (
            ConfigKey::ApiUrl,
            "openhuman.config_update_model_settings",
            json!({"api_url": "value"}),
        ),
        (
            ConfigKey::InferenceUrl,
            "openhuman.config_update_model_settings",
            json!({"inference_url": "value"}),
        ),
        (
            ConfigKey::DefaultModel,
            "openhuman.config_update_model_settings",
            json!({"default_model": "value"}),
        ),
        (
            ConfigKey::AutonomyLevel,
            "openhuman.config_update_autonomy_settings",
            json!({"level": "value"}),
        ),
        (
            ConfigKey::PrivacyMode,
            "openhuman.config_set_privacy_mode",
            json!({"mode": "value"}),
        ),
    ];
    for (key, expected_method, expected_params) in cases {
        let (method, params) = config_update(key, "value".to_string());
        assert_eq!(method, expected_method);
        assert_eq!(params, expected_params);
    }
}
