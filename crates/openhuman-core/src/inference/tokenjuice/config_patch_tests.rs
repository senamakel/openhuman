use super::*;

#[test]
fn applies_only_present_fields() {
    let mut cfg = TokenjuiceConfig::default();
    let patch: TokenjuiceSettingsPatch =
        serde_json::from_str(r#"{ "ccr_min_tokens": 1200, "search_enabled": false }"#).unwrap();
    patch.apply(&mut cfg);
    assert_eq!(cfg.ccr_min_tokens, 1200);
    assert!(!cfg.search_enabled);
    // Untouched fields keep defaults.
    assert!(cfg.router_enabled);
    assert!(cfg.code_enabled);
}

#[test]
fn repl_switches_can_be_flipped() {
    let mut cfg = TokenjuiceConfig::default();
    let patch: TokenjuiceSettingsPatch =
        serde_json::from_str(r#"{ "repl_handle_enabled": false }"#).unwrap();
    patch.apply(&mut cfg);
    assert!(!cfg.repl_handle_enabled);
}

#[test]
fn removed_plain_copy_setting_is_ignored_by_existing_settings_updates() {
    let mut cfg = TokenjuiceConfig::default();
    let patch: TokenjuiceSettingsPatch =
        serde_json::from_str(r#"{ "repl_save_enabled": true }"#).unwrap();
    patch.apply(&mut cfg);
    assert!(cfg.repl_handle_enabled);
    assert!(!cfg.ccr_disk_enabled);
}

#[test]
fn ttl_can_be_set_and_cleared() {
    let mut cfg = TokenjuiceConfig::default();
    let set: TokenjuiceSettingsPatch = serde_json::from_str(r#"{ "ccr_ttl_secs": 300 }"#).unwrap();
    set.apply(&mut cfg);
    assert_eq!(cfg.ccr_ttl_secs, Some(300));
    // 0 clears the TTL.
    let clear: TokenjuiceSettingsPatch = serde_json::from_str(r#"{ "ccr_ttl_secs": 0 }"#).unwrap();
    clear.apply(&mut cfg);
    assert_eq!(cfg.ccr_ttl_secs, None);
}
