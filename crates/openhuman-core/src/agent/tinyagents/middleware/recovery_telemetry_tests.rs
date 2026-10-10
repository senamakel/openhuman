use super::*;
use tinytools_jev::recovery::{RecoveryAbstention, RecoveryClass, RecoveryDecision};

#[test]
fn comparison_metadata_preserves_validated_numbers_without_payloads() {
    let config = RecoveryConfig {
        classifier: RecoveryClassifier::Compare,
        ..Default::default()
    };
    let advice = RecoveryAdvice::Classified {
        class: RecoveryClass::WrongTool,
        decision: RecoveryDecision {
            answers: [
                (
                    "class".into(),
                    RecoveryAnswer::Choice {
                        probabilities: Default::default(),
                        confidence: 0.9,
                    },
                ),
                ("recoverability".into(), RecoveryAnswer::Noul(0.8)),
            ]
            .into_iter()
            .collect(),
            input_tokens: Some(100),
            output_tokens: Some(20),
            latency: Default::default(),
            attempts: 1,
        },
        alternate: Some("private_tool_identifier".into()),
        correction_score: None,
    };
    let event = metadata(&config, "accepted", Some(&advice));
    assert_eq!(event.mode, "compare");
    assert_eq!(event.class, Some("wrong_tool"));
    assert_eq!(event.confidence, Some(0.9));
    assert_eq!(event.recoverability, Some(0.8));
    assert_eq!(event.attempts, Some(1));
    assert!(event.alternate);
    assert_eq!(event.input_tokens, Some(100));
    assert!(!format!("{event:?}").contains("private_tool_identifier"));
}
#[test]
fn fallback_metadata_has_no_invented_provider_results() {
    let config = RecoveryConfig::default();
    for advice in [
        None,
        Some(RecoveryAdvice::Abstained(
            RecoveryAbstention::EvaluatorFailure,
        )),
    ] {
        let event = metadata(&config, "fallback", advice.as_ref());
        assert_eq!(event.class, None);
        assert_eq!(event.confidence, None);
        assert_eq!(event.recoverability, None);
        assert_eq!(event.attempts, None);
        assert_eq!(event.input_tokens, None);
        assert!(!event.alternate);
    }
}
#[test]
fn unexpected_version_payload_is_not_logged() {
    let config = RecoveryConfig {
        threshold_version: "secret value\nsecond line".into(),
        ..Default::default()
    };
    assert_eq!(
        metadata(&config, "fallback", None).threshold_version,
        "invalid"
    );
}
