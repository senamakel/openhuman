//! Translate a parsed classifier decision into side effects.
//!
//! ## Commit 1 scope
//!
//! Only publishes [`crate::core::event_bus::DomainEvent::TriggerEvaluated`]
//! and logs the would-be action. No sub-agents are actually dispatched
//! yet — `react` / `escalate` get a `tracing::info!` saying "would
//! dispatch to …" and nothing else happens. This keeps commit 1
//! bounded to the classifier round-trip so we can validate the
//! triage turn works end-to-end without also wiring in the
//! sub-agent runner (which requires additional plumbing covered in
//! commit 2).
//!
//! ## Commit 2 scope
//!
//! - `acknowledge` → write a memory note via the memory domain.
//! - `react` → call `subagent_runner::run_subagent(trigger_reactor_def, prompt, …)`
//!   directly, bypassing the `spawn_subagent` tool path.
//! - `escalate` → same but against the existing `orchestrator`
//!   built-in. Also publishes [`TriggerEscalated`].
//!
//! `drop` is terminal in both commits.

use super::decision::TriageAction;
use super::envelope::TriggerEnvelope;
use super::evaluator::TriageRun;
use super::events;

/// Interpret a [`TriageRun`] and fire the matching side effects.
///
/// In commit 1 this is mostly just logging + `TriggerEvaluated` —
/// there is no provider work and no subagent dispatch, so the function
/// is infallible. The signature returns `Result` anyway because
/// commit 2 adds fallible paths (memory writes, `run_subagent`
/// failures) and we want the caller's error handling in place from
/// the start.
pub async fn apply_decision(run: TriageRun, envelope: &TriggerEnvelope) -> anyhow::Result<()> {
    // Always publish `TriggerEvaluated` — it's the single source of
    // truth for dashboards, counts every trigger regardless of action.
    events::publish_evaluated(
        envelope,
        run.decision.action.as_str(),
        run.used_local,
        run.latency_ms,
    );

    match run.decision.action {
        TriageAction::Drop => {
            tracing::debug!(
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                reason = %run.decision.reason,
                "[triage::escalation] DROP — no downstream work"
            );
        }
        TriageAction::Acknowledge => {
            tracing::info!(
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                reason = %run.decision.reason,
                "[triage::escalation] ACKNOWLEDGE — commit 2 will persist a memory note"
            );
            // Commit 2: write memory note via the memory domain.
        }
        TriageAction::React | TriageAction::Escalate => {
            // The parser already enforced that target_agent + prompt
            // are set for these variants — unwraps here would be
            // safe, but we still handle the absent case as a defensive
            // measure in case a future refactor weakens the parser.
            let target = run.decision.target_agent.as_deref().unwrap_or("<unknown>");
            let prompt = run.decision.prompt.as_deref().unwrap_or("");
            tracing::info!(
                action = %run.decision.action.as_str(),
                target_agent = %target,
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                prompt_chars = prompt.chars().count(),
                reason = %run.decision.reason,
                "[triage::escalation] {action} — commit 2 will run {target} sub-agent",
                action = run.decision.action.as_str().to_uppercase(),
                target = target,
            );
            // Commit 2: `subagent_runner::run_subagent(def, prompt, …)` here,
            // then `events::publish_escalated(envelope, target)`.
        }
    }
    Ok(())
}
