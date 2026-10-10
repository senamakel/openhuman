//! Bounded host accounting and advice projection. TinyTools owns evaluation logic.
use super::call_effect::CallEffect;
use crate::agent::tinyagents::host::OpenHumanRunContext;
use crate::config::schema::{RecoveryClassifier, RecoveryConfig};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tinyagents_harness::context::RunContext;
use tinytools_jev::recovery::{
    RecoveryAdvice, RecoveryAdviser, RecoveryAlternateReason, RecoveryAnswer, RecoveryClass,
    RecoveryEffect, RecoveryObservation, RecoveryPhase,
};

pub(super) struct RunRecovery {
    config: RecoveryConfig,
    adviser: Option<RecoveryAdviser>,
    evaluations: AtomicU32,
    gate: tokio::sync::Semaphore,
    ledger: Mutex<HashMap<String, u32>>,
    candidates: Mutex<Vec<tinytools_jev::JevOption>>,
    failed_tools: Mutex<HashSet<String>>,
}
impl RunRecovery {
    pub(super) fn new(config: &crate::config::Config) -> Self {
        let settings = config.agent.recovery.clone();
        let adviser = settings.validate().ok().and_then(|_| {
            if !matches!(
                settings.classifier,
                RecoveryClassifier::Compare | RecoveryClassifier::Jev
            ) {
                return None;
            }
            let factory =
                crate::agent::tinyagents::recovery_provider::installed_recovery_provider()?;
            RecoveryAdviser::new(factory(config)?, settings.thresholds()).ok()
        });
        Self {
            config: settings,
            adviser,
            evaluations: AtomicU32::new(0),
            gate: tokio::sync::Semaphore::new(1),
            ledger: Mutex::new(HashMap::new()),
            candidates: Mutex::default(),
            failed_tools: Mutex::default(),
        }
    }
    #[cfg(test)]
    pub(super) fn with_evaluator(
        config: RecoveryConfig,
        evaluator: std::sync::Arc<dyn tinytools_jev::recovery::RecoveryEvaluator>,
    ) -> Self {
        let adviser = RecoveryAdviser::new(evaluator, config.thresholds()).ok();
        Self {
            config,
            adviser,
            evaluations: AtomicU32::new(0),
            gate: tokio::sync::Semaphore::new(1),
            ledger: Mutex::default(),
            candidates: Mutex::default(),
            failed_tools: Mutex::default(),
        }
    }
    pub(super) fn set_candidates(&self, candidates: Vec<tinytools_jev::JevOption>) {
        if let Ok(mut slot) = self.candidates.lock() {
            *slot = candidates.into_iter().take(16).collect();
        }
    }
    pub(super) fn record_failure(&self, tool: &str) {
        if !matches!(
            self.config.classifier,
            RecoveryClassifier::Jev | RecoveryClassifier::Compare
        ) {
            return;
        }
        if let Ok(mut failed) = self.failed_tools.lock() {
            failed.insert(tool.to_owned());
        }
    }
    fn take_decision(&self) -> bool {
        self.evaluations
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.config.max_decisions_per_run).then_some(n + 1)
            })
            .is_ok()
    }
    pub(super) fn clear(&self, scope: &str) {
        if let Ok(mut ledger) = self.ledger.lock() {
            ledger.remove(scope);
        }
    }
    /// Return a lower ceiling or a request-only nudge. Never increases headroom.
    /// Unknown failures are always fed to the existing tracker by the caller.
    pub(super) async fn advise(
        &self,
        ctx: &RunContext<OpenHumanRunContext>,
        scope: &str,
        tool: &str,
        shape: String,
        failure: &str,
        effect: CallEffect,
    ) -> (bool, Option<String>) {
        let started = std::time::Instant::now();
        let record = |outcome: &str, advice: Option<&RecoveryAdvice>| {
            super::recovery_telemetry::record(&self.config, started, outcome, advice)
        };
        if self.config.validate().is_err() {
            record("invalid_config", None);
            return (false, None);
        }
        if !matches!(
            self.config.classifier,
            RecoveryClassifier::Compare | RecoveryClassifier::Jev
        ) {
            return (false, None);
        }
        let compare = self.config.classifier == RecoveryClassifier::Compare;
        // Stable key is operation/scope only, independent of model class and args.
        // Compare's private observation ledger never enters either host tracker
        // or policy budgets; it only makes the evaluated evidence comparable.
        let count = self
            .ledger
            .lock()
            .map(|mut ledger| {
                let n = ledger.entry(scope.into()).or_default();
                *n = n.saturating_add(1);
                *n
            })
            .unwrap_or(0);
        let Some(adviser) = &self.adviser else {
            record("no_provider", None);
            return (false, None);
        };
        // Executed writes have uncertain outcomes; do not ask a classifier to
        // justify repeating them. The generic ladder remains authoritative.
        if effect != CallEffect::ReadOnly {
            record("effect_unsafe", None);
            return (false, None);
        }
        let Ok(_permit) = self.gate.try_acquire() else {
            record("concurrency_limit", None);
            return (false, None);
        };
        if ctx
            .remaining_wall_clock()
            .is_some_and(|remaining| remaining.is_zero())
        {
            record("run_deadline", None);
            return (false, None);
        }
        if !self.take_decision() {
            record("decision_limit", None);
            return (false, None);
        }
        let ceiling = std::time::Duration::from_millis(self.config.decision_timeout_ms);
        let deadline = ctx
            .remaining_wall_clock()
            .map_or(ceiling, |d| d.min(ceiling));
        let mut observation = RecoveryObservation::new(
            RecoveryPhase::Unknown,
            format!("Capability: {tool}"),
            failure,
        );
        observation.argument_shape = shape;
        observation.effect = RecoveryEffect::ReadOnly;
        observation.attempts = count;
        observation.repeated_failures = count;
        // Both batches share one wall-clock budget. The first answer can enable
        // wrong-tool advice; a repeated blocker can enable it independently.
        let expires = tokio::time::Instant::now() + deadline;
        let (advice, outcome) = tokio::select! {
            biased;
            _ = ctx.cancellation.cancelled() => (None, "cancelled"),
            result = tokio::time::timeout_at(expires, adviser.advise(observation)) => match result {
                Err(_) => (None, "timeout"),
                Ok(Err(_)) => (None, "invalid_decision"),
                Ok(Ok(advice)) => (Some(advice), "evaluated"),
            },
        };
        record(outcome, advice.as_ref());
        let Some(RecoveryAdvice::Classified {
            class, decision, ..
        }) = advice
        else {
            return (false, None);
        };
        let recoverable = matches!(decision.answers.get("recoverability"), Some(RecoveryAnswer::Noul(value)) if *value >= self.config.recoverability);
        let mut alternate = None;
        if self.config.alternate_tools
            && (class == RecoveryClass::WrongTool || count >= self.config.repeated_blocker)
        {
            let failed = self
                .failed_tools
                .lock()
                .map(|f| f.clone())
                .unwrap_or_default();
            let candidates = self
                .candidates
                .lock()
                .map(|c| {
                    c.iter()
                        .filter(|c| c.key != tool && !failed.contains(&c.key))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let expired = tokio::time::Instant::now() >= expires;
            let permitted = !candidates.is_empty() && !expired && self.take_decision();
            if !candidates.is_empty() && !permitted {
                record(
                    if expired {
                        "decision_deadline"
                    } else {
                        "decision_limit"
                    },
                    None,
                );
            }
            if permitted {
                let mut observation = RecoveryObservation::new(
                    RecoveryPhase::Unknown,
                    format!("Capability: {tool}"),
                    failure,
                );
                observation.effect = RecoveryEffect::ReadOnly;
                observation.attempts = count;
                observation.repeated_failures = count;
                observation.candidates = candidates;
                observation.alternate_reason = Some(if class == RecoveryClass::WrongTool {
                    RecoveryAlternateReason::WrongToolAdvice
                } else {
                    RecoveryAlternateReason::RepeatedBlocker
                });
                let (response, outcome) = tokio::select! {
                    biased;
                    _ = ctx.cancellation.cancelled() => (None, "cancelled"),
                    result = tokio::time::timeout_at(expires, adviser.advise(observation)) => match result {
                        Err(_) => (None, "timeout"),
                        Ok(Err(_)) => (None, "invalid_decision"),
                        Ok(Ok(advice)) => (Some(advice), "evaluated"),
                    },
                };
                record(outcome, response.as_ref());
                if let Some(RecoveryAdvice::Classified {
                    alternate: choice, ..
                }) = response
                {
                    alternate = choice;
                }
            }
        }
        if compare {
            return (false, None);
        }
        let optional_refusal = super::failure_policy::is_optional_service(tool)
            && matches!(
                class,
                RecoveryClass::Authentication
                    | RecoveryClass::Permission
                    | RecoveryClass::Unavailable
            );
        let halt = count >= 3
            || (optional_refusal && count >= 2)
            || (!optional_refusal
                && matches!(
                    class,
                    RecoveryClass::Authentication
                        | RecoveryClass::Permission
                        | RecoveryClass::Unsupported
                ))
            || (!optional_refusal && class == RecoveryClass::Unavailable && !recoverable);
        let nudge = (!halt).then(|| if optional_refusal {
            format!("Advisory recovery for `{tool}`: this optional service cannot currently be used. Continue with other available capabilities through the normal tool gates; do not resend the failed operation.")
        } else { match class {
            RecoveryClass::WrongArguments if recoverable => format!("Advisory recovery for `{tool}`: inspect its parameter schema and choose a corrected call through the normal tool gates. Do not resend the failed call unchanged."),
            RecoveryClass::WrongTool => format!("Advisory recovery for `{tool}`: reconsider whether this capability addresses the task; choose your next step through the normal tool gates."),
            _ => format!("Advisory recovery for `{tool}`: reassess the failed observation before choosing your next step. Existing permissions and retry limits still apply."),
        }});
        let nudge = nudge.map(|n| match alternate {
            Some(name) => format!("{n} Consider the already available `{name}` capability; any call must pass the normal admission checks."),
            None => n,
        });
        (halt, nudge)
    }
}

/// Arguments never cross the decision boundary: expose bounded field types only.
pub(super) fn argument_shape(value: &serde_json::Value) -> String {
    value
        .as_object()
        .map(|m| {
            m.iter()
                .take(16)
                .map(|(name, value)| {
                    let kind = match value {
                        serde_json::Value::Null => "null",
                        serde_json::Value::Bool(_) => "boolean",
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Object(_) => "object",
                    };
                    // Keys can themselves be data. Admit schema-like ASCII identifiers only.
                    let name = if name.len() <= 64
                        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    {
                        name.as_str()
                    } else {
                        "field"
                    };
                    format!("{name}:{kind}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// Only a short scrubbed diagnostic sentence is eligible. JSON/HTML/URLs and
/// multiline payloads cannot be reliably minimized here and cause abstention.
pub(super) fn diagnostic(text: &str) -> Option<String> {
    let first = text.lines().find(|l| !l.trim().is_empty())?.trim();
    if first.len() > 240 || first.contains(['{', '[', '<', '"', '=']) || first.contains("://") {
        return None;
    }
    let safe = crate::security::scrub::sanitize_text(first).value;
    if safe.is_empty() {
        None
    } else {
        Some(safe)
    }
}

/// A conservative snapshot of the registered callable session surface. Only
/// self-contained declared reads need no new credentials, roots or approval.
/// The next model call still enters the ordinary live authorization path.
pub(super) fn admitted_candidates(
    registry: &tinyagents_harness::tool::ToolRegistry<(), OpenHumanRunContext>,
    session: Option<&crate::tools::agent_policy::ToolPolicySession>,
) -> Vec<tinytools_jev::JevOption> {
    registry
        .model_callable_names()
        .into_iter()
        .filter_map(|name| {
            if name == "none"
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
            {
                return None;
            }
            if session.is_some_and(|s| {
                !s.is_allowed(&name)
                    || s.hidden_tool_names.contains(&name)
                    || !matches!(
                        s.decision_for(&name).action,
                        crate::tools::agent_policy::ToolPolicyAction::Allow
                    )
            }) {
                return None;
            }
            let tool = registry.get(&name)?;
            let policy = tool.policy();
            if !policy.classified
                || !policy.side_effects.read_only
                || policy.side_effects.external_service
                || tool.permission_level() != tinytools::PermissionLevel::ReadOnly
                || policy.access.approval_required
                || !policy.access.credentials.is_empty()
                || policy.access.workspace != tinytools::WorkspaceAccess::None
            {
                return None;
            }
            let description = diagnostic(tool.description())?;
            Some(tinytools_jev::JevOption {
                key: name,
                description,
            })
        })
        .take(16)
        .collect()
}
