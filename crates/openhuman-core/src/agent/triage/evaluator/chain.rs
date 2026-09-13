//! The tiered cloud → retry → local fallback orchestration.

use std::future::Future;
use std::time::Duration;

use anyhow::Context;

use crate::config::Config;
use crate::cron::scheduler_gate::LlmPermit;

use super::super::envelope::TriggerEnvelope;
use super::super::events;
use super::super::routing::{
    build_local_provider_with_config, resolve_provider_with_config, ResolvedProvider,
};
use super::arm::{try_arm, ArmError};
use super::outcome::{TriageOutcome, TriageResolutionPath};

/// Cap on how long to wait for a server-supplied `Retry-After` before
/// giving up on the cloud arm and falling through to local. Mirrors
/// the cap in `ReliableProvider::compute_backoff`.
const RETRY_AFTER_CAP: Duration = Duration::from_millis(30_000);

/// Default backoff for transient (non-rate-limit) cloud failures
/// before the single retry. Short enough to keep tail latency
/// bounded; long enough for a wedged TCP connection to give up.
const TRANSIENT_BACKOFF: Duration = Duration::from_millis(500);

/// How far in the future a Deferred outcome asks the caller to retry.
/// A short tick mirrors the issue's "next tick retries the whole
/// chain" language — long enough to shed a thundering herd, short
/// enough that user-visible latency on transient outages stays in the
/// tens of seconds.
const DEFER_WAKEUP_MS: i64 = 30_000;

/// Run the triage classifier with the full tiered fallback chain.
///
/// 1. Resolve the cloud provider.
/// 2. Try cloud; on 429 / transient, sleep and retry once.
/// 3. On a second 429 / transient, build the local provider and
///    fall back to it (acquiring the global LLM permit).
/// 4. On local failure, return `TriageOutcome::Deferred` so the
///    caller (typically a trigger-handler RPC) can reschedule.
pub async fn run_triage(envelope: &TriggerEnvelope) -> anyhow::Result<TriageOutcome> {
    let config = Config::load_or_init()
        .await
        .context("loading config for triage turn")?;
    let cloud = resolve_provider_with_config(&config)
        .await
        .context("resolving provider for triage turn")?;
    let local = build_local_provider_with_config(&config);

    let outcome = run_triage_with_arms_inner(cloud, local, envelope, || {
        crate::cron::scheduler_gate::wait_for_capacity()
    })
    .await;
    if let Err(err) = &outcome {
        events::publish_failed(envelope, &format!("{err}"));
    }
    outcome
}

/// Production entry point that takes already-resolved arms and acquires
/// the global LLM permit via [`scheduler_gate::wait_for_capacity`].
///
/// Use [`run_triage_with_arms_for_test`] in tests to bypass the shared
/// semaphore. This function is `pub` for integration callers outside
/// this module that supply pre-resolved providers.
pub async fn run_triage_with_arms(
    cloud: ResolvedProvider,
    local: Option<ResolvedProvider>,
    envelope: &TriggerEnvelope,
) -> anyhow::Result<TriageOutcome> {
    run_triage_with_arms_inner(cloud, local, envelope, || {
        crate::cron::scheduler_gate::wait_for_capacity()
    })
    .await
}

/// Test-only entry point: skip the global LLM permit acquisition so the
/// triage tests don't contend with `scheduler_gate`'s process-wide
/// 1-slot semaphore or get trapped by a stale `Paused` policy left in
/// `STATE` by another test's `init_global` call.
#[cfg(test)]
pub async fn run_triage_with_arms_for_test(
    cloud: ResolvedProvider,
    local: Option<ResolvedProvider>,
    envelope: &TriggerEnvelope,
) -> anyhow::Result<TriageOutcome> {
    run_triage_with_arms_inner(cloud, local, envelope, || async { None }).await
}

/// Core implementation of the tiered cloud→retry→local fallback.
///
/// `acquire_permit` is called exactly once, on the local-fallback arm,
/// to obtain the global LLM permit. Production callers pass
/// `scheduler_gate::wait_for_capacity`; tests pass `|| async { None }`
/// to skip the shared semaphore.
async fn run_triage_with_arms_inner<F, Fut>(
    cloud: ResolvedProvider,
    local: Option<ResolvedProvider>,
    envelope: &TriggerEnvelope,
    acquire_permit: F,
) -> anyhow::Result<TriageOutcome>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<LlmPermit>>,
{
    // Track whether the cloud arm bailed because of user budget so the
    // eventual Deferred reason explains *why* we're sitting idle rather
    // than the generic "both arms failed" copy.
    let mut cloud_budget_exhausted: Option<anyhow::Error> = None;
    // Track whether the cloud arm bailed because the prompt-guard
    // flagged the content (OPENHUMAN-TAURI-X). The guard runs before
    // dispatch and is shared across arms, so a flag on cloud will
    // repeat on local — surface the verdict in the Deferred reason
    // for operator-facing telemetry.
    let mut cloud_safety_flagged: Option<anyhow::Error> = None;

    // ── Cloud arm ──────────────────────────────────────────────────
    match try_arm(&cloud, envelope, TriageResolutionPath::Cloud).await {
        Ok(run) => return Ok(TriageOutcome::Decision(run)),
        Err(ArmError::Fatal(err)) => return Err(err),
        Err(ArmError::BudgetExhausted(err)) => {
            tracing::warn!(
                source = %envelope.source.slug(),
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                path = TriageResolutionPath::Cloud.as_str(),
                error = %err,
                "[triage::evaluator] cloud rejected for budget; \
                 skipping retry and falling back to local arm"
            );
            cloud_budget_exhausted = Some(err);
        }
        Err(ArmError::SafetyFlagged(err)) => {
            tracing::warn!(
                source = %envelope.source.slug(),
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                path = TriageResolutionPath::Cloud.as_str(),
                error = %err,
                "[triage::evaluator] cloud rejected by prompt-guard; \
                 skipping retry and falling back to local arm"
            );
            cloud_safety_flagged = Some(err);
        }
        Err(ArmError::Retryable { retry_after_ms, .. }) => {
            // Sleep before the cloud retry. Honour Retry-After when
            // present; otherwise use a short backoff so the second
            // attempt has a real chance of finding the upstream
            // recovered.
            let sleep_ms = retry_after_ms
                .map(|ms| Duration::from_millis(ms).min(RETRY_AFTER_CAP))
                .unwrap_or(TRANSIENT_BACKOFF);
            tracing::info!(
                sleep_ms = sleep_ms.as_millis() as u64,
                had_retry_after = retry_after_ms.is_some(),
                "[triage::evaluator] cloud retry pending after retryable failure"
            );
            tokio::time::sleep(sleep_ms).await;

            match try_arm(&cloud, envelope, TriageResolutionPath::CloudAfterRetry).await {
                Ok(run) => return Ok(TriageOutcome::Decision(run)),
                Err(ArmError::Fatal(err)) => return Err(err),
                Err(ArmError::BudgetExhausted(err)) => {
                    tracing::warn!(
                        source = %envelope.source.slug(),
                        label = %envelope.display_label,
                        external_id = %envelope.external_id,
                        path = TriageResolutionPath::CloudAfterRetry.as_str(),
                        error = %err,
                        "[triage::evaluator] cloud rejected for budget on retry; \
                         falling back to local arm"
                    );
                    cloud_budget_exhausted = Some(err);
                }
                Err(ArmError::SafetyFlagged(err)) => {
                    tracing::warn!(
                        source = %envelope.source.slug(),
                        label = %envelope.display_label,
                        external_id = %envelope.external_id,
                        path = TriageResolutionPath::CloudAfterRetry.as_str(),
                        error = %err,
                        "[triage::evaluator] cloud rejected by prompt-guard on retry; \
                         falling back to local arm"
                    );
                    cloud_safety_flagged = Some(err);
                }
                Err(ArmError::Retryable { .. }) => {
                    // Exhausted cloud budget — fall through to local.
                    tracing::warn!(
                        "[triage::evaluator] cloud retry budget exhausted; \
                         falling back to local arm"
                    );
                }
            }
        }
    }

    // ── Local fallback ─────────────────────────────────────────────
    let Some(local) = local else {
        // No local arm available at all (runtime disabled, no model
        // configured) — the only honest outcome is a deferral so the
        // next tick retries the whole chain.
        //
        // `reason` is part of `TriageOutcome::Deferred` and may be
        // forwarded into telemetry / UI, so it must stay a stable,
        // scrubbed string. Raw upstream error text goes to the debug
        // log instead, where it is operator-visible but not surfaced.
        let reason = if let Some(err) = cloud_safety_flagged.as_ref() {
            tracing::debug!(
                target: "[triage::evaluator]",
                source = %envelope.source.slug(),
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                error = %err,
                "prompt-guard rejected on cloud; no local arm — full guard verdict"
            );
            "prompt-guard rejection; local arm unavailable".to_string()
        } else if let Some(err) = cloud_budget_exhausted.as_ref() {
            tracing::debug!(
                target: "[triage::evaluator]",
                source = %envelope.source.slug(),
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                error = %err,
                "cloud budget exhausted; no local arm — full upstream error"
            );
            "cloud budget exhausted; local arm unavailable".to_string()
        } else {
            "cloud retry exhausted; local arm unavailable".to_string()
        };
        return Ok(TriageOutcome::Deferred {
            defer_until_ms: now_ms().saturating_add(DEFER_WAKEUP_MS),
            reason,
        });
    };

    // Hold the global LLM permit for the lifetime of the local turn —
    // protects laptop RAM from concurrent local model calls (#1073).
    let _gate_permit = acquire_permit().await;

    match try_arm(&local, envelope, TriageResolutionPath::LocalFallback).await {
        Ok(run) => Ok(TriageOutcome::Decision(run)),
        Err(ArmError::Fatal(err))
        | Err(ArmError::BudgetExhausted(err))
        | Err(ArmError::SafetyFlagged(err))
        | Err(ArmError::Retryable { source: err, .. }) => {
            // Local also failed — defer rather than surface a hard
            // error. Today's "hard fail" is the wrong default for a
            // transient blocker per #1257.
            //
            // `reason` is part of the public Deferred outcome and may
            // flow into telemetry / UI, so keep it scrubbed. Raw error
            // text from cloud + local lives in the structured warn
            // fields below — visible to operators, not callers.
            let reason = if cloud_safety_flagged.is_some() {
                "prompt-guard rejection; local arm also failed".to_string()
            } else if cloud_budget_exhausted.is_some() {
                "cloud budget exhausted; local arm also failed".to_string()
            } else {
                "cloud retry exhausted; local arm also failed".to_string()
            };
            tracing::warn!(
                target: "[triage::evaluator]",
                source = %envelope.source.slug(),
                label = %envelope.display_label,
                external_id = %envelope.external_id,
                local_error = %err,
                cloud_error = cloud_budget_exhausted
                    .as_ref()
                    .or(cloud_safety_flagged.as_ref())
                    .map(|e| e.to_string())
                    .unwrap_or_default(),
                defer_ms = DEFER_WAKEUP_MS,
                reason = %reason,
                "both arms failed; deferring"
            );
            Ok(TriageOutcome::Deferred {
                defer_until_ms: now_ms().saturating_add(DEFER_WAKEUP_MS),
                reason,
            })
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
