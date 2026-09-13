//! Deterministic provider/config/policy rejection classification: custom
//! OpenAI-compatible upstream envelopes, provider access-policy denials,
//! backend-owned error codes, external content-moderation rejections, and
//! provider configuration rejections (unknown model, bad temperature, …).

use super::*;

/// Whether a custom OpenAI-compatible proxy returned the known generic
/// upstream 400 envelope:
/// `{"error":{"message":"Bad request to upstream provider","type":"upstream_error","status":400}}`.
///
/// This shape is deterministic provider/user-state (endpoint-model mismatch,
/// unsupported schema, provider-side validation) and does not provide
/// actionable signal for OpenHuman Sentry triage.
pub fn is_custom_openai_upstream_bad_request_http_400(
    provider: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> bool {
    if provider != "custom_openai" || status != reqwest::StatusCode::BAD_REQUEST {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("bad request to upstream provider") && lower.contains("upstream_error")
}

pub fn log_custom_openai_upstream_bad_request_http_400(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "provider_user_state",
        reason = "custom_openai_upstream_bad_request",
        "[llm_provider] {operation} custom_openai upstream 400 — not reporting to Sentry"
    );
}

/// Whether a provider non-2xx response is a deterministic provider-policy
/// denial (not a product bug) that should be demoted from Sentry.
///
/// Canonical example: Kimi's coding endpoint rejects non-agent clients with
/// HTTP 403 + `access_terminated_error` and a message like:
/// "currently only available for Coding Agents …".
pub fn is_provider_access_policy_denied_http_403(status: reqwest::StatusCode, body: &str) -> bool {
    if status != reqwest::StatusCode::FORBIDDEN {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("access_terminated_error")
        || lower.contains("currently only available for coding agents")
}

pub fn log_provider_access_policy_denied_http_403(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "provider_access_policy",
        "[llm_provider] {operation} provider access-policy 403 — not reporting to Sentry"
    );
}

/// Whether this provider response carries a managed-backend `errorCode` (#870)
/// that the backend already owns — so the FE must not double-report (F2/F4).
///
/// Gated on `provider == `[`openhuman_backend_model::PROVIDER_LABEL`]: an `errorCode`
/// is only trustworthy on the **managed backend**. A BYO / direct-provider body
/// that merely contains an `errorCode`-shaped field must NOT be treated as
/// backend-owned (CodeRabbit) — those keep reaching Sentry via the status gate.
///
/// Returns `false` for a backend-flagged **malformed** `BAD_REQUEST`: that one
/// `errorCode` case is a client-built payload the backend couldn't parse, and
/// the FE *does* page for it (F8). Delegates to the single-source decision in
/// [`crate::inference::provider::backend_error_code_skips_sentry`]
/// so the provider layer, the higher-layer re-report classifier, and the
/// Sentry `before_send` filter can't drift.
pub fn is_backend_error_code_owned(provider: &str, body: &str) -> bool {
    provider == openhuman_backend_model::PROVIDER_LABEL
        && crate::inference::provider::backend_error_code_skips_sentry(body)
}

pub fn log_backend_error_code_owned(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
    body: &str,
) {
    let code =
        crate::inference::provider::extract_backend_error_code_token(body).unwrap_or_default();
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "backend_error_code",
        error_code = %code,
        "[llm_provider] {operation} backend errorCode={code} ({status}) — backend owns \
         this error, not reporting to Sentry"
    );
}

/// Whether a provider non-2xx response is an **external content-moderation
/// rejection** — the user routes their provider (here: ollama) through a
/// third-party safety/moderation proxy that refuses the prompt with a `400`
/// and a verdict envelope, e.g.
/// `{"error":"Message rejected by Ombudsman","score":80}` (TAURI-RUST-ECR,
/// 4,517 events from a single looping triage-agent machine).
///
/// This is `genuinely-unpreventable` from OpenHuman's side: the request is
/// well-formed, the rejection comes from an external guard we neither own nor
/// configure, and there is no client lever to reshape the request into one the
/// proxy will accept. The triage agent re-issues the same prompt every turn, so
/// the raw 400 floods `report_error` (400 ∉ the transient set in
/// [`super::dispatch::should_report_provider_http_failure`]). Demote to an
/// info log while the error still propagates so retry/fallback runs
/// unchanged — the same classify-and-backpressure answer as the 5MV / A3T /
/// 8S3 precedent.
///
/// Anchored on the moderation-verdict shape — the rejection wording
/// (`message rejected` / `ombudsman`) or the `"score"` verdict field — none of
/// which OpenHuman's own backend or a normal provider 400 (malformed request,
/// schema error) emits, so a genuine bug still reaches Sentry. Covered by a
/// verbatim-body test so a proxy wording drift fails CI instead of silently
/// leaking events.
pub fn is_provider_moderation_rejection_http_400(status: reqwest::StatusCode, body: &str) -> bool {
    if status != reqwest::StatusCode::BAD_REQUEST {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("message rejected")
        || lower.contains("ombudsman")
        // The moderation proxy returns its confidence as a `"score"` JSON field
        // alongside the verdict; anchor on the quoted key shape (not a bare
        // `score`) so an unrelated 400 mentioning the word in prose isn't
        // swallowed.
        || lower.contains("\"score\"")
}

pub fn log_provider_moderation_rejection(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "external_moderation_rejection",
        "[llm_provider] {operation} external content-moderation rejection ({status}) — request \
         refused by a third-party moderation proxy (no client lever), not reporting to Sentry"
    );
}

/// Whether a provider non-2xx response is a deterministic
/// **configuration-rejection** user-state error (unknown model id,
/// abstract tier leaked to a custom provider, model-specific temperature
/// constraint) that should be demoted from Sentry to an info log.
///
/// Provider-aware (inverted polarity vs. the 401/403 backend rule): for
/// most config-rejection phrases the same body from the OpenHuman
/// **backend** stays Sentry-actionable — that would mean we sent our own
/// backend a bad request (a regression, e.g. #2079). Restricted to the
/// observed shapes (400 invalid-param / unknown-model, 404
/// model-does-not-exist, 422 unprocessable); 408/429 are transient and
/// handled separately.
///
/// **Exception: OpenAI-compatible "unknown model"** (`Model 'X' is not
/// available. Use GET /openai/v1/models …`). The OpenHuman backend now
/// emits this exact body for user-configured unknown model ids, so it is
/// user-state regardless of provider — the polarity guard is dropped for
/// this specific shape (TAURI-RUST-2Z1). See
/// [`super::is_openai_compatible_unknown_model_message`].
pub fn is_provider_config_rejection_http(
    status: reqwest::StatusCode,
    provider: &str,
    body: &str,
) -> bool {
    // 403 is included for the Ollama Cloud subscription gate:
    // `{"error":"this model requires a subscription, upgrade for access: …"}`.
    // That is deterministic user-state (paid-tier model, free account) — the
    // same class as the 400/404/422 config-rejection shapes above. See
    // TAURI-RUST-4XK. The general `is_backend_auth_failure` polarity guard
    // still fires first (backend 401/403 → SessionExpired), so this branch
    // is only reachable for non-backend providers. The phrase-level polarity
    // guard below (`provider != openhuman_backend_model::PROVIDER_LABEL`) provides
    // a second layer of defence for the non-OpenAI-compat shapes.
    if !matches!(status.as_u16(), 400 | 403 | 404 | 422) {
        return false;
    }
    if !crate::inference::provider::is_provider_config_rejection_message(body) {
        return false;
    }
    // OpenAI-compatible "unknown model" body is user-state regardless of
    // provider — both third-party `custom_openai` upstreams and our own
    // OpenHuman backend now emit it for user-configured model ids that
    // aren't in the registry (TAURI-RUST-2Z1).
    if crate::inference::provider::is_openai_compatible_unknown_model_message(body) {
        return true;
    }
    // Remaining config-rejection phrases (DeepSeek `supported api model
    // names are`, Moonshot `invalid temperature`, litellm envelopes, …)
    // are intrinsically scoped to third-party providers — keep the
    // polarity guard so a regression where our own backend emits one of
    // those still reaches Sentry.
    provider != openhuman_backend_model::PROVIDER_LABEL
}

pub fn log_provider_config_rejection(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "provider_config_rejection",
        "[llm_provider] {operation} provider config-rejection ({status}) — \
         user model/param configuration, not reporting to Sentry"
    );
}
