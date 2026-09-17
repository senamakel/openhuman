//! The `api_error` entry point: reads a failed HTTP response, runs it through
//! every classifier in this module in priority order, demotes the ones that
//! are expected user/provider state, and reports the rest to Sentry.

use super::*;

/// Whether a non-2xx provider response is worth reporting to Sentry.
///
/// Transient upstream statuses — 429 Too Many Requests, 408 Request Timeout,
/// and 502/503/504 gateway-layer failures — are caller-side throttling or
/// upstream-capacity signals. The reliable-provider layer already retries
/// with backoff and falls back across providers/models, and the aggregate
/// "all providers exhausted" event still fires if every attempt fails.
/// Reporting each individual transient failure floods Sentry (see
/// OPENHUMAN-TAURI-6Y / 2E / 84 / T: thousands of events/day per user from
/// a single upstream rate-limit / outage window). Callers should still
/// propagate the error so retry and fallback logic runs unchanged; this
/// only gates the per-attempt Sentry report.
pub fn should_report_provider_http_failure(status: reqwest::StatusCode) -> bool {
    !crate::core::observability::TRANSIENT_PROVIDER_HTTP_STATUSES.contains(&status.as_u16())
}

/// Build a sanitized provider error from a failed HTTP response.
///
/// Reports the failure to Sentry with `provider` and `status` tags so
/// upstream LLM errors are visible in observability without every call-site
/// having to remember to log — except for:
///
/// - **Transient statuses** (429 — see [`should_report_provider_http_failure`]).
///   These get retried by the reliable-provider layer and don't deserve a
///   per-attempt Sentry event.
/// - **401/403 from the OpenHuman backend provider** — the user's app session
///   expired. That is expected user-state, not a server bug, and reporting it
///   spams Sentry (OPENHUMAN-TAURI-1T: 5,414 events from a single user whose
///   cron loops kept firing post-expiry). Instead we publish a
///   [`crate::core::events::DomainEvent::SessionExpired`] so the credentials
///   subscriber clears the session and flips the scheduler-gate signed-out
///   override, halting downstream LLM work. 401/403 from **other** providers
///   (OpenAI, Anthropic, …) still go to Sentry — those mean a misconfigured
///   API key, which is actionable.
/// - **Provider config-rejection** (4xx unknown-model / abstract-tier /
///   model-specific temperature) from a **non-backend** provider — the
///   user pointed a custom provider at a model/param it doesn't accept.
///   Deterministic user-config state, surfaced in the UI; demoted to an
///   info log (#2079 / #2076 / #2202). See
///   [`is_provider_config_rejection_http`].
pub async fn api_error(provider: &str, response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let status_str = status.as_u16().to_string();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read provider error body>".to_string());
    let sanitized = sanitize_api_error(&body);
    let message = format!("{provider} API error ({status}): {sanitized}");

    let is_auth_failure = matches!(status.as_u16(), 401 | 403);
    let is_backend = provider == openhuman_backend_model::PROVIDER_LABEL;
    let is_budget_exhausted_user_state = is_budget_exhausted_http_400(status, &body);
    // Local inference server (LM Studio etc.) running with no model loaded —
    // pure local user-state, nothing we sent is malformed. Demote and replace
    // the body with actionable "load a model" guidance (TAURI-RUST-DMQ, mirrors
    // the embeddings #3688 special-case).
    let is_local_provider_no_model_loaded = is_local_provider_no_model_loaded(status, &body);
    let is_custom_openai_upstream_bad_request =
        is_custom_openai_upstream_bad_request_http_400(provider, status, &body);
    let is_provider_access_policy_denied = is_provider_access_policy_denied_http_403(status, &body);
    let is_provider_config_rejection = is_provider_config_rejection_http(status, provider, &body);
    // Context-overflow is status-agnostic: match the body directly (some
    // custom gateways mis-report it as 500 — TAURI-RUST-501 — so a status
    // gate would let those through to `should_report_provider_http_failure`).
    let is_context_window_exceeded = is_context_window_exceeded_message(&body);
    // Monthly-quota exhaustion is likewise status-agnostic: the Kiro IDE proxy
    // wraps its 402 inside a 500 envelope (TAURI-RUST-C9A), so match the body
    // directly rather than gating on a 402 status (which the credits matcher
    // below does). The user's third-party plan quota is spent — no local lever.
    let is_quota_exhausted = is_provider_quota_exhausted(&body);
    // F4/F2: any managed-backend response carrying a stable `errorCode` is
    // backend-owned — it already paged or is expected user-state — so the FE
    // must not double-report. The one exception (malformed `BAD_REQUEST`) is
    // excluded by `is_backend_error_code_owned` and falls through to the
    // status gate below, which reports it (status 400 is non-transient) — F8.
    let is_backend_error_code_owned = is_backend_error_code_owned(provider, &body);
    // Missing/invalid BYO API key on a non-backend provider — user-config
    // state, not a product bug. Demote from Sentry (TAURI-RUST-DHM flood).
    let is_byo_auth_failure = is_byo_provider_auth_failure_http(provider, status, &body);
    // OpenAI ChatGPT/Codex OAuth access token expired with no usable refresh
    // token — user must reconnect OpenAI. Deterministic user-state, demote
    // from Sentry (TAURI-RUST-8FQ flood).
    let is_openai_oauth_session_expired =
        is_openai_oauth_session_expired_http(provider, status, &body);
    // Insufficient-credits 402: the user's own BYO provider account is out of
    // balance — a flat billing fact, not a reservation-window error, so there is
    // NO local max_tokens lever to apply. Demote from Sentry like the per-method
    // compatible-provider arms; the complete classification for a genuinely-
    // unpreventable BYO-balance condition (TAURI-RUST-4QF DeepSeek "Insufficient
    // Balance"). This shared helper backs the two methods that delegate here
    // (chat_via_responses fallback and the non-streaming completion path).
    let is_insufficient_credits_402 = is_provider_insufficient_credits_402(status, &body);
    // Ollama Cloud hosted-inference 500 (`Internal Server Error (ref: <uuid>)`):
    // provider-internal, non-deterministic, no client lever. Demote from Sentry
    // and replace the opaque ref body with actionable guidance (TAURI-RUST-5MV).
    let is_ollama_cloud_internal_500 = is_ollama_cloud_internal_500(provider, status, &body);
    // External content-moderation proxy ("Ombudsman") refused the prompt with a
    // 400 + verdict — well-formed request, external safety guard, no client
    // lever. Demote from Sentry like the native_chat ladder (TAURI-RUST-ECR).
    let is_moderation_rejection = is_provider_moderation_rejection_http_400(status, &body);

    if is_auth_failure && is_backend {
        // Single source of truth for backend session-expiry handling (warn +
        // SessionExpired publish + final-string sanitize) — shared with the
        // hand-rolled `chat_completions` chain in `compatible.rs`.
        publish_backend_session_expired("api_error", provider, status, &message);
    } else if is_budget_exhausted_user_state {
        log_budget_exhausted_http_400("api_error", provider, None, status);
    } else if is_local_provider_no_model_loaded {
        log_local_provider_no_model_loaded("api_error", provider, None, status);
    } else if is_custom_openai_upstream_bad_request {
        log_custom_openai_upstream_bad_request_http_400("api_error", provider, None, status);
    } else if is_provider_access_policy_denied {
        log_provider_access_policy_denied_http_403("api_error", provider, None, status);
    } else if is_provider_config_rejection {
        log_provider_config_rejection("api_error", provider, None, status);
    } else if is_context_window_exceeded {
        log_context_window_exceeded("api_error", provider, None, status);
    } else if is_quota_exhausted {
        log_provider_quota_exhausted("api_error", provider, None, status);
    } else if is_backend_error_code_owned {
        log_backend_error_code_owned("api_error", provider, None, status, &body);
    } else if is_byo_auth_failure {
        log_byo_provider_auth_failure("api_error", provider, None, status);
    } else if is_openai_oauth_session_expired {
        log_openai_oauth_session_expired("api_error", provider, None, status);
    } else if is_insufficient_credits_402 {
        log_provider_insufficient_credits_402("api_error", provider, None, status);
    } else if is_ollama_cloud_internal_500 {
        log_ollama_cloud_internal_500("api_error", provider, None, status);
    } else if is_moderation_rejection {
        log_provider_moderation_rejection("api_error", provider, None, status);
    } else if should_report_provider_http_failure(status) {
        crate::core::observability::report_error(
            message.as_str(),
            "llm_provider",
            "api_error",
            &[
                ("provider", provider),
                ("status", status_str.as_str()),
                ("failure", "non_2xx"),
            ],
        );
    }
    // Replace the opaque `Internal Server Error (ref: <uuid>)` body with
    // actionable guidance; the prefix anchors the higher-layer re-report
    // demotion (`is_ollama_cloud_internal_500_message`).
    if is_ollama_cloud_internal_500 {
        return anyhow::anyhow!(ollama_cloud_internal_500_user_message(None, status));
    }
    // Replace the raw `No models loaded` body with actionable guidance so the
    // surfaced chat error tells the user how to recover (TAURI-RUST-DMQ).
    if is_local_provider_no_model_loaded {
        return anyhow::anyhow!(local_provider_no_model_loaded_user_message());
    }
    anyhow::anyhow!(message)
}
