use super::*;

#[test]
fn debug_never_prints_the_key() {
    let key = ApiKey::from("th_live_supersecret");
    let debug = format!("{key:?}");
    assert!(!debug.contains("supersecret"), "leaked: {debug}");
    assert!(debug.contains("redacted"));
}

#[test]
fn blankness_is_detected_after_trimming() {
    assert!(ApiKey::from("   ").is_blank());
    assert!(ApiKey::from("").is_blank());
    assert!(!ApiKey::from(" th_x ").is_blank());
    assert_eq!(ApiKey::from(" th_x ").expose(), "th_x");
}
