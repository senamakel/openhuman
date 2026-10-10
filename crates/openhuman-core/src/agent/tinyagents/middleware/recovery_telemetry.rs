//! Payload-free evidence for advisory evaluation and observational compare mode.
use crate::config::schema::{RecoveryClassifier, RecoveryConfig};
use std::time::Instant;
use tinytools_jev::recovery::{RecoveryAdvice, RecoveryAnswer};

#[derive(Debug, PartialEq)]
pub(super) struct RecoveryTelemetry<'a> {
    pub mode: &'static str,
    pub threshold_version: &'a str,
    pub outcome: &'a str,
    pub class: Option<&'static str>,
    pub confidence: Option<f64>,
    pub recoverability: Option<f64>,
    pub attempts: Option<u32>,
    pub alternate: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

pub(super) fn metadata<'a>(
    config: &'a RecoveryConfig,
    outcome: &'a str,
    advice: Option<&RecoveryAdvice>,
) -> RecoveryTelemetry<'a> {
    let mode = match config.classifier {
        RecoveryClassifier::Keywords => "keywords",
        RecoveryClassifier::Compare => "compare",
        RecoveryClassifier::Jev => "jev",
        RecoveryClassifier::Off => "off",
    };
    let threshold_version = if config.threshold_version.len() <= 128
        && config
            .threshold_version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        &config.threshold_version
    } else {
        "invalid"
    };
    let mut result = RecoveryTelemetry {
        mode,
        threshold_version,
        outcome,
        class: None,
        confidence: None,
        recoverability: None,
        attempts: None,
        alternate: false,
        input_tokens: None,
        output_tokens: None,
    };
    if let Some(RecoveryAdvice::Classified {
        class,
        decision,
        alternate,
        ..
    }) = advice
    {
        result.class = Some(class.key());
        if let Some(RecoveryAnswer::Choice { confidence, .. }) = decision.answers.get("class") {
            result.confidence = Some(*confidence);
        }
        if let Some(RecoveryAnswer::Noul(value)) = decision.answers.get("recoverability") {
            result.recoverability = Some(*value);
        }
        result.attempts = Some(decision.attempts);
        result.alternate = alternate.is_some();
        result.input_tokens = decision.input_tokens;
        result.output_tokens = decision.output_tokens;
    }
    result
}

pub(super) fn record(
    config: &RecoveryConfig,
    started: Instant,
    outcome: &str,
    advice: Option<&RecoveryAdvice>,
) {
    let event = metadata(config, outcome, advice);
    let abstention = match advice {
        Some(RecoveryAdvice::Abstained(reason)) => Some(format!("{reason:?}")),
        _ => None,
    };
    tracing::info!(target: "openhuman::recovery",
        source = "jev", mode = event.mode, request_version = "tool-recovery-v1",
        threshold_version = event.threshold_version, outcome = event.outcome,
        class = event.class, confidence = event.confidence, recoverability = event.recoverability,
        elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        attempts = event.attempts, alternate = event.alternate,
        input_tokens = event.input_tokens, output_tokens = event.output_tokens,
        abstention = abstention.as_deref(),
        "tool recovery evaluation");
}

#[cfg(test)]
#[path = "recovery_telemetry_tests.rs"]
mod tests;
