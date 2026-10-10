use super::*;

#[test]
fn recovery_defaults_to_keywords_and_rejects_invalid_policy() {
    let config = RecoveryConfig::default();
    assert_eq!(config.classifier, RecoveryClassifier::Keywords);
    assert!(config.validate().is_ok());
    for input in [
        r#"{"decision_timeout_ms":3001}"#,
        r#"{"class_confidence":1.2}"#,
        r#"{"jev_route":"typo"}"#,
        r#"{"max_decisions_per_run":0}"#,
    ] {
        assert!(
            serde_json::from_str::<RecoveryConfig>(input).is_err(),
            "{input}"
        );
    }
    let mut invalid = config;
    invalid.recoverability = f64::NAN;
    assert!(invalid.validate().is_err());
}
