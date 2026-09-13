//! Budget, quota, and per-request rate-cap classification — the deterministic
//! "third-party account/plan is out of runway" family of provider errors.

/// Whether a provider non-2xx response is a deterministic budget-exhausted
/// user-state error that should be demoted from Sentry to an info log.
pub fn is_budget_exhausted_http_400(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST
        && crate::inference::provider::is_budget_exhausted_message(body)
}

pub fn log_budget_exhausted_http_400(
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
        kind = "budget",
        "[llm_provider] {operation} budget-exhausted 400 — not reporting to Sentry"
    );
}

/// Whether a provider non-2xx response is a deterministic
/// **insufficient-credits** user-state error — the BYO provider account
/// (e.g. OpenRouter) lacks the balance to satisfy the request.
///
/// This is the *residual* case once the request already caps `max_tokens`
/// (so the provider's pre-flight is priced against a realistic output budget
/// rather than the model's full window — see
/// [`crate::inference::provider::ChatRequest::max_tokens`]): a 402
/// that still arrives means the user's own third-party account is genuinely
/// out of credit, a billing state OpenHuman has no lever over. Demote from
/// Sentry to an info log rather than page once per retry
/// (TAURI-RUST-C62: 12k events from a single low-balance user).
///
/// Gated on the 402 status **and** a credit/payment phrase so an unrelated
/// 402 is not swallowed. The phrase list is covered by a verbatim-body test
/// so a provider wording drift fails CI instead of silently leaking events.
pub fn is_provider_insufficient_credits_402(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::PAYMENT_REQUIRED && body_indicates_insufficient_credits(body)
}

/// Phrase-level matcher for an insufficient-credits / out-of-balance provider
/// error body. Single source of truth for the credit-phrase set, shared by the
/// emit-site guard [`is_provider_insufficient_credits_402`] (which adds the 402
/// status gate) and the `before_send` defense-in-depth filter
/// [`crate::core::observability::is_insufficient_credits_event`] (which matches
/// the formatted `<provider> API error (402 …): <body>` message so the demotion
/// reaches every compatible-provider HTTP path — `chat_with_system`,
/// `chat_with_history`, the streaming gates, and `api_error` — not just
/// `Provider::chat()`'s `native_chat` cascade). TAURI-RUST-C62.
pub fn body_indicates_insufficient_credits(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("requires more credits")
        || lower.contains("more credits")
        || lower.contains("can only afford")
        || lower.contains("insufficient credit")
        || lower.contains("insufficient balance")
        || lower.contains("insufficient funds")
        || lower.contains("payment required")
}

pub fn log_provider_insufficient_credits_402(
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
        kind = "insufficient_credits",
        "[llm_provider] {operation} provider insufficient-credits 402 — BYO account out of \
         balance (no local lever), not reporting to Sentry"
    );
}

/// Whether a provider non-2xx response is a deterministic **monthly-quota /
/// usage-limit exhausted** user-state error — the user's third-party plan has
/// spent its allotment for the period and no request will succeed until it
/// resets (a billing/plan state OpenHuman has no lever over).
///
/// Distinct from [`is_provider_insufficient_credits_402`] in two ways:
/// 1. The signal is a *usage-quota cap* ("you have reached the limit",
///    `MONTHLY_REQUEST_COUNT`), not an account balance.
/// 2. The upstream proxy may wrap its own 402 inside a **500** envelope, e.g.
///    Kiro IDE: `kiro API error (500 Internal Server Error): {"error":\
///    {"message":"HTTP 402 from Kiro IDE: {\"reason\":\"MONTHLY_REQUEST_COUNT\"}"…}}`.
///    So this is **status-agnostic** — matched against the body like
///    [`super::context_window::is_context_window_exceeded_message`] — because
///    gating on a 402 transport status (as the credits matcher does) would let
///    the 500-wrapped flood straight through to
///    [`super::dispatch::should_report_provider_http_failure`]
///    (TAURI-RUST-C9A: 9k events from a single quota-capped user, retried per
///    memory-extraction attempt).
///
/// Keyed on quota-specific wording only, so a generic 500 outage (or a 429
/// rate-limit, which has its own transient handling) is not swallowed. Covered
/// by a verbatim-body test so a provider wording drift fails CI.
pub fn is_provider_quota_exhausted(body: &str) -> bool {
    body_indicates_quota_exhausted(body)
}

/// Phrase-level matcher for a provider monthly-quota / usage-limit exhausted
/// body. Single source of truth for the quota-phrase set, shared by the
/// emit-site guard [`is_provider_quota_exhausted`] and the `before_send`
/// defense-in-depth filter
/// [`crate::core::observability::is_quota_exhausted_event`] (which matches the
/// formatted `<provider> API error (…): <body>` message so the demotion reaches
/// every compatible-provider HTTP path, not just `Provider::chat()`'s
/// `native_chat` cascade). TAURI-RUST-C9A.
pub fn body_indicates_quota_exhausted(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("monthly_request_count")
        || lower.contains("monthly request")
        || lower.contains("monthly limit")
        || lower.contains("monthly quota")
        || lower.contains("quota exceeded")
        || lower.contains("usage limit exceeded")
        // Codex/ChatGPT OAuth `/responses` plan-cap body (TAURI-RUST-AFE):
        // `usage_limit_reached` / "The usage limit has been reached" — a plan
        // quota with no "monthly"/"quota" co-marker, so the phrases above miss
        // it. Both are quota-specific enough to match on their own (the loop
        // retries until `resets_at`, flooding from a single capped Plus user).
        || lower.contains("usage_limit_reached")
        || lower.contains("usage limit has been reached")
        // "reached the limit" alone is ambiguous (rate-limit, token-limit), so
        // require a quota/plan/request/monthly co-marker to keep the blast
        // radius on plan-quota exhaustion only.
        || (lower.contains("reached the limit")
            && (lower.contains("request")
                || lower.contains("quota")
                || lower.contains("monthly")
                || lower.contains("plan")))
}

pub fn log_provider_quota_exhausted(
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
        kind = "quota_exhausted",
        "[llm_provider] {operation} provider monthly-quota exhausted — third-party plan limit \
         reached (no local lever), not reporting to Sentry"
    );
}

/// Whether a provider error body is a **permanent per-request rate-cap
/// rejection**: the provider refused because a *single* request's token count
/// exceeds the account's tokens-per-minute (TPM) budget, so no amount of
/// retrying or spacing can ever let it through on the current tier.
///
/// Distinct from a *transient* TPM `429` ("rate limit reached … try again in
/// 2s" — a burst that
/// [`super::context_window::is_context_window_exceeded_message`] and the
/// `reliable` retry classifier deliberately keep retryable), from a
/// monthly-plan quota ([`body_indicates_quota_exhausted`]), and from
/// context-window overflow
/// ([`super::context_window::is_context_window_exceeded_message`], a
/// model-size limit not a rate cap). Here the request is larger than the
/// per-minute limit outright, so it is permanently non-viable until the user
/// picks a higher-tier model/provider — OpenHuman has no lever to raise a
/// third-party account's TPM tier.
///
/// Canonical wire shape (groq `on_demand` free tier, Sentry TAURI-RUST-HXF):
/// `groq API error (413 Payload Too Large): {"error":{"message":"Request too
/// large for model `openai/gpt-oss-120b` in organization `org_…` service tier
/// `on_demand` on tokens per minute (TPM): Limit 8000, Requested 42084 …"}}`.
///
/// Anchored on BOTH the permanence marker `"request too large"` (a single
/// request over the cap, not a burst) AND a per-minute-tokens marker
/// (`"tokens per minute"` / `"(tpm)"`), so a transient "rate limit reached,
/// retry in Ns" burst — which lacks "request too large" — is NOT swallowed and
/// stays retryable + Sentry-visible. Status-agnostic (groq uses `413`; a
/// gateway could wrap it) and covered by a verbatim-body test so a provider
/// wording drift fails CI. Single source of truth shared by
/// [`crate::core::observability::is_provider_user_state_message`] (Sentry
/// demotion of the `domain=agent` re-report).
pub fn is_provider_rate_cap_exceeded_message(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("request too large")
        && (lower.contains("tokens per minute") || lower.contains("(tpm)"))
}
