//! Store-time `GET /auth/me` session validation: transient-failure
//! classification, the retry/timeout budget, and the small payload helpers
//! `store_session_inner` uses while resolving what to persist.

use serde_json::{json, Value};
use std::time::Duration;

use crate::api::jwt::decode_jwt_exp;
use crate::api::rest::BackendOAuthClient;

const AUTH_ME_STORE_RETRY_DELAY: Duration = Duration::from_millis(150);
const AUTH_ME_STORE_TRANSIENT_STATUSES: &[u16] = &[408, 429, 500, 502, 503, 504, 520];

/// Wall-clock budget for the store-time `GET /auth/me` validation (issue #5166).
///
/// The shared backend client allows a 120s request timeout + 15s connect timeout
/// (`api::rest`), but the desktop sign-in RPC that drives `auth_store_session`
/// gives up far sooner — `AUTH_STORE_TIMEOUT_MS` (25s) × `AUTH_STORE_RETRIES` in
/// `desktopDeepLinkListener.ts`. If the backend is reachable but slow, that 120s
/// ceiling lets `/auth/me` hang past the frontend's patience: the RPC times out
/// and bounces a genuinely-authenticated user back to sign-in *before* the
/// deferred-validation fallback in `store_session_inner` ever gets a chance to
/// fire (the exact `auth_me_timeout` bounce in Sentry `TAURI-REACT-1V`).
///
/// Capping store-time validation well under the frontend budget makes a slow
/// backend fail *fast* into the caller-authorized pending-session path (for a
/// live-`exp` JWT), so the user lands in the app with deferred revalidation
/// instead of being bounced. Overridable via `OPENHUMAN_AUTH_ME_STORE_TIMEOUT_MS`
/// for ops tuning and tests.
pub(crate) const AUTH_ME_STORE_VALIDATION_BUDGET: Duration = Duration::from_secs(12);
pub(crate) const AUTH_ME_STORE_VALIDATION_BUDGET_ENV: &str = "OPENHUMAN_AUTH_ME_STORE_TIMEOUT_MS";

/// Store-time `GET /auth/me` budget resolver. Reads the
/// `OPENHUMAN_AUTH_ME_STORE_TIMEOUT_MS` override (positive integer milliseconds),
/// otherwise the `AUTH_ME_STORE_VALIDATION_BUDGET` default.
pub(crate) fn auth_me_store_validation_budget() -> Duration {
    std::env::var(AUTH_ME_STORE_VALIDATION_BUDGET_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(AUTH_ME_STORE_VALIDATION_BUDGET)
}

/// Validate the freshly minted session token against `GET /auth/me`, bounded by
/// `auth_me_store_validation_budget()`. On budget exhaustion returns a
/// transient-classified timeout error so `store_session_inner` routes a
/// live-`exp` JWT into the deferred-validation fallback rather than hanging until
/// the desktop sign-in RPC times out and bounces the user (issue #5166).
pub(super) async fn fetch_current_user_for_session_store(
    client: &BackendOAuthClient,
    token: &str,
) -> Result<Value, String> {
    let budget = auth_me_store_validation_budget();
    match tokio::time::timeout(
        budget,
        fetch_current_user_for_session_store_inner(client, token),
    )
    .await
    {
        Ok(result) => result,
        Err(_elapsed) => {
            // Message must contain a `TRANSIENT_TRANSPORT_PHRASES` phrase
            // ("timeout") so `auth_me_store_failure_is_transient` buckets it as
            // transient and the deferred-validation path can take over.
            let reason = format!(
                "GET /auth/me validation timeout after {}ms (store-time budget exceeded)",
                budget.as_millis()
            );
            tracing::warn!(
                domain = "credentials",
                operation = "fetch_current_user_for_session_store",
                budget_ms = budget.as_millis() as u64,
                "[credentials][auth-store] {reason}"
            );
            Err(reason)
        }
    }
}

async fn fetch_current_user_for_session_store_inner(
    client: &BackendOAuthClient,
    token: &str,
) -> Result<Value, String> {
    match client.fetch_current_user(token).await {
        Ok(user) => Ok(user),
        Err(first) => {
            let first_reason = format!("{first:#}");
            if !auth_me_store_failure_is_transient(&first_reason) {
                return Err(first_reason);
            }

            tokio::time::sleep(AUTH_ME_STORE_RETRY_DELAY).await;
            tracing::debug!(
                domain = "credentials",
                operation = "fetch_current_user_for_session_store",
                reason = %first_reason,
                "[credentials][auth-store] retrying GET /auth/me after transient failure"
            );
            client
                .fetch_current_user(token)
                .await
                .map_err(|second| format!("{second:#}"))
        }
    }
}

pub(crate) fn auth_me_store_failure_is_transient(reason: &str) -> bool {
    if let Some(status) = auth_me_failure_status(reason) {
        return AUTH_ME_STORE_TRANSIENT_STATUSES.contains(&status);
    }

    crate::core::observability::contains_transient_transport_phrase(reason)
}

fn auth_me_failure_status(reason: &str) -> Option<u16> {
    let lower = reason.to_ascii_lowercase();
    (100..600).find(|status| {
        let status = status.to_string();
        lower.contains(&format!("({status}"))
            || lower.contains(&format!("http {status}"))
            || lower.contains(&format!("status {status}"))
            || lower.contains(&format!("status code {status}"))
    })
}

pub(super) fn jwt_exp_live_at(
    token: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let exp = decode_jwt_exp(token)?;
    (exp > now).then_some(exp)
}

pub(super) fn fallback_session_user_for_deferred_validation() -> Value {
    json!({ "pendingBackendValidation": true })
}

pub(crate) fn sanitize_stored_session_user(
    user: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match user {
        Some(serde_json::Value::Object(map)) if map.is_empty() => None,
        Some(serde_json::Value::Null) => None,
        other => other,
    }
}

pub(crate) fn normalize_local_session_user(
    user: serde_json::Value,
    local_user_id: &str,
) -> serde_json::Value {
    let mut map = match user {
        serde_json::Value::Object(map) => map,
        other => return other,
    };
    map.insert(
        "id".to_string(),
        serde_json::Value::String(local_user_id.to_string()),
    );
    map.insert(
        "_id".to_string(),
        serde_json::Value::String(local_user_id.to_string()),
    );
    serde_json::Value::Object(map)
}
