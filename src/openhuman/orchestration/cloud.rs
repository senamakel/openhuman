//! Cloud event pusher — forwards sanitized orchestration events up to the
//! hosted brain (`POST /orchestration/v1/events`).
//!
//! Phase 0 runs this in **shadow mode**: the local wake graph remains
//! authoritative and the push is an additional, best-effort, fire-and-forget
//! side effect gated behind [`OrchestrationConfig::cloud_shadow`]
//! (default off). It never blocks or fails ingest.
//!
//! Auth + base-URL plumbing mirrors the other hosted-API adapters
//! (`announcements/ops.rs`, `billing/ops.rs`): an app-session JWT via
//! `require_live_session_token`, the backend base via `effective_backend_api_url`,
//! and one shared `BackendOAuthClient`.

use std::time::Duration;

use reqwest::Method;

use crate::api::config::effective_backend_api_url;
use crate::api::BackendOAuthClient;
use crate::openhuman::config::Config;

use super::wire::{OrchestrationEventEnvelopeWire, WorldDiffBatchWire};

const LOG: &str = "orchestration";
const EVENTS_PATH: &str = "/orchestration/v1/events";
const WORLD_DIFF_PATH: &str = "/orchestration/v1/world-diff";

/// Jittered retry schedule for a transient push failure (3 retries after the
/// first attempt). Matches the plan's 1s/4s/10s cadence.
const DEFAULT_BACKOFFS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(4),
    Duration::from_secs(10),
];

/// Push one sanitized event to the hosted brain. Resolves the app-session JWT
/// and backend base, then POSTs with bounded retry. Returns `Err` only after
/// the retry budget is exhausted (or the session is signed out).
pub async fn push_event(
    config: &Config,
    envelope: &OrchestrationEventEnvelopeWire,
) -> Result<(), String> {
    let token = crate::openhuman::credentials::session_support::require_live_session_token(config)?;
    let api_url = effective_backend_api_url(&config.api_url);
    let client = BackendOAuthClient::new(&api_url).map_err(|e| e.to_string())?;
    push_event_with(&client, &token, envelope, &DEFAULT_BACKOFFS).await
}

/// Upload a batch of world-diff entries — the subconscious tier's primary
/// trigger. Same auth/base/retry plumbing as [`push_event`]. Returns `Err` only
/// after the retry budget is exhausted (or the session is signed out).
pub async fn push_world_diff(config: &Config, batch: &WorldDiffBatchWire) -> Result<(), String> {
    if batch.entries.is_empty() {
        return Ok(());
    }
    let token = crate::openhuman::credentials::session_support::require_live_session_token(config)?;
    let api_url = effective_backend_api_url(&config.api_url);
    let client = BackendOAuthClient::new(&api_url).map_err(|e| e.to_string())?;
    post_with_retry(
        &client,
        &token,
        WORLD_DIFF_PATH,
        batch.to_value(),
        &DEFAULT_BACKOFFS,
        &format!(
            "world-diff session={} entries={}",
            batch.session_id,
            batch.entries.len()
        ),
    )
    .await
}

/// Inner push with an injectable client, token, and backoff schedule so the
/// transport can be exercised against a mock server without real credentials or
/// real sleeps (`backoffs = &[]` → single attempt). Public for integration
/// tests (`tests/orchestration_shadow_push_e2e.rs`).
pub async fn push_event_with(
    client: &BackendOAuthClient,
    token: &str,
    envelope: &OrchestrationEventEnvelopeWire,
    backoffs: &[Duration],
) -> Result<(), String> {
    let label = format!(
        "event session={} seq={}",
        envelope.session_id, envelope.event.seq
    );
    post_with_retry(
        client,
        token,
        EVENTS_PATH,
        envelope.to_value(),
        backoffs,
        &label,
    )
    .await
}

/// Generic authed POST with bounded jittered-backoff retry. Shared by every
/// orchestration uplink (`events`, `world-diff`). `backoffs = &[]` → one attempt.
async fn post_with_retry(
    client: &BackendOAuthClient,
    token: &str,
    path: &str,
    body: serde_json::Value,
    backoffs: &[Duration],
    label: &str,
) -> Result<(), String> {
    let mut attempt: usize = 0;
    loop {
        match client
            .authed_json(token, Method::POST, path, Some(body.clone()))
            .await
        {
            Ok(_) => {
                log::debug!(target: LOG, "[orchestration] cloud.push.ok {label} attempt={}", attempt + 1);
                return Ok(());
            }
            Err(err) => {
                let msg = crate::api::flatten_authed_error(err);
                if attempt >= backoffs.len() {
                    log::warn!(target: LOG, "[orchestration] cloud.push.give_up {label} attempts={} err={msg}", attempt + 1);
                    return Err(msg);
                }
                log::warn!(target: LOG, "[orchestration] cloud.push.retry {label} attempt={} err={msg}", attempt + 1);
                tokio::time::sleep(backoffs[attempt]).await;
                attempt += 1;
            }
        }
    }
}

/// World-diff uploader with injectable client/token/backoffs for tests.
pub async fn push_world_diff_with(
    client: &BackendOAuthClient,
    token: &str,
    batch: &WorldDiffBatchWire,
    backoffs: &[Duration],
) -> Result<(), String> {
    let label = format!(
        "world-diff session={} entries={}",
        batch.session_id,
        batch.entries.len()
    );
    post_with_retry(
        client,
        token,
        WORLD_DIFF_PATH,
        batch.to_value(),
        backoffs,
        &label,
    )
    .await
}

// Transport tests live in `tests/orchestration_shadow_push_e2e.rs` (integration
// crate): the root crate's `cfg(test)` build is currently blocked by unrelated
// stale test modules at this checkout, so the pusher is exercised over wiremock
// from an integration test that links the compiled lib instead.
