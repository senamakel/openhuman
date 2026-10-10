//! Opt-in advisory recovery policy; keywords remain the default.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Selects whether unresolved failures receive remote advisory evaluation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryClassifier {
    #[default]
    /// Preserve the existing deterministic keyword fallback.
    Keywords,
    /// Evaluate observationally without changing model-facing behavior.
    Compare,
    /// Apply validated advice within the existing execution limits.
    Jev,
    /// Disable decision assistance while keeping existing failure guards.
    Off,
}

/// Bounded host policy for the `[agent.recovery]` configuration block.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, try_from = "RecoveryConfigWire")]
pub struct RecoveryConfig {
    /// Evaluation mode; defaults to keywords.
    pub classifier: RecoveryClassifier,
    /// Optional route override; otherwise inherits the tool-search route.
    pub jev_route: Option<String>,
    /// Optional endpoint origin override for the selected route.
    pub jev_base_url: Option<String>,
    /// Total advisory deadline in milliseconds, at most 3000 and remaining run time.
    pub decision_timeout_ms: u64,
    /// Maximum provider requests during one run, including alternate evaluation.
    pub max_decisions_per_run: u32,
    /// Allow advisory selection among already admitted read-only capabilities.
    pub alternate_tools: bool,
    /// Identifies the provisional threshold policy for evaluation records.
    pub threshold_version: String,
    /// Minimum class distribution concentration; not measured correctness.
    pub class_confidence: f64,
    /// Minimum positive recovery probability for corrective or alternate advice.
    pub recoverability: f64,
    /// Minimum concentration for optional correction or alternate answers.
    pub advice_confidence: f64,
    /// Host-counted failures needed for repeated-blocker alternate eligibility.
    pub repeated_blocker: u32,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            classifier: RecoveryClassifier::Keywords,
            jev_route: None,
            jev_base_url: None,
            decision_timeout_ms: 3000,
            max_decisions_per_run: 8,
            alternate_tools: false,
            threshold_version: "provisional-v1".into(),
            class_confidence: 0.75,
            recoverability: 0.75,
            advice_confidence: 0.75,
            repeated_blocker: 2,
        }
    }
}

impl RecoveryConfig {
    /// Validate both deserialized and directly constructed policy values.
    pub fn validate(&self) -> Result<(), String> {
        if self.recoverability <= 0.0
            || self.decision_timeout_ms == 0
            || self.decision_timeout_ms > 3000
            || self.max_decisions_per_run == 0
            || self.max_decisions_per_run > 64
            || self.repeated_blocker == 0
            || self.repeated_blocker > 64
            || self.threshold_version.trim().is_empty()
            || self.threshold_version.len() > 128
            || [
                self.class_confidence,
                self.recoverability,
                self.advice_confidence,
            ]
            .iter()
            .any(|p| !p.is_finite() || !(0.0..=1.0).contains(p))
        {
            return Err("invalid bounded recovery policy".into());
        }
        if self
            .jev_route
            .as_deref()
            .is_some_and(|r| !matches!(r, "auto" | "tinyhumans" | "typesafe" | "openrouter"))
        {
            return Err("invalid recovery Jev route".into());
        }
        Ok(())
    }

    /// Project the host thresholds into the provider-neutral TinyTools policy.
    pub fn thresholds(&self) -> tinytools_jev::recovery::RecoveryThresholds {
        tinytools_jev::recovery::RecoveryThresholds {
            version: self.threshold_version.clone(),
            class_confidence: self.class_confidence,
            recoverability: self.recoverability,
            advice_confidence: self.advice_confidence,
            repeated_blocker: self.repeated_blocker,
        }
    }
}

// Deserialize through an unvalidated wire type so every config loading path
// enforces the same bounds. Runtime callers still call validate().
#[derive(Deserialize, JsonSchema)]
#[serde(default)]
struct RecoveryConfigWire {
    classifier: RecoveryClassifier,
    jev_route: Option<String>,
    jev_base_url: Option<String>,
    decision_timeout_ms: u64,
    max_decisions_per_run: u32,
    alternate_tools: bool,
    threshold_version: String,
    class_confidence: f64,
    recoverability: f64,
    advice_confidence: f64,
    repeated_blocker: u32,
}
impl Default for RecoveryConfigWire {
    fn default() -> Self {
        let c = RecoveryConfig::default();
        Self {
            classifier: c.classifier,
            jev_route: c.jev_route,
            jev_base_url: c.jev_base_url,
            decision_timeout_ms: c.decision_timeout_ms,
            max_decisions_per_run: c.max_decisions_per_run,
            alternate_tools: c.alternate_tools,
            threshold_version: c.threshold_version,
            class_confidence: c.class_confidence,
            recoverability: c.recoverability,
            advice_confidence: c.advice_confidence,
            repeated_blocker: c.repeated_blocker,
        }
    }
}
impl TryFrom<RecoveryConfigWire> for RecoveryConfig {
    type Error = String;
    fn try_from(c: RecoveryConfigWire) -> Result<Self, Self::Error> {
        let config = Self {
            classifier: c.classifier,
            jev_route: c.jev_route,
            jev_base_url: c.jev_base_url,
            decision_timeout_ms: c.decision_timeout_ms,
            max_decisions_per_run: c.max_decisions_per_run,
            alternate_tools: c.alternate_tools,
            threshold_version: c.threshold_version,
            class_confidence: c.class_confidence,
            recoverability: c.recoverability,
            advice_confidence: c.advice_confidence,
            repeated_blocker: c.repeated_blocker,
        };
        config.validate()?;
        Ok(config)
    }
}
