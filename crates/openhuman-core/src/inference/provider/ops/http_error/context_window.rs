//! Context-window overflow classification: the request exceeded the
//! configured model's context length.

/// Whether a provider error body indicates the request exceeded the model's
/// context window (the conversation/prompt is too long for the configured
/// model). This is a deterministic user-state / usage condition — the
/// remediation is "start a new chat, trim the conversation, or pick a
/// larger-context model" — not a product bug. Sentry has no signal to act
/// on.
///
/// Single source of truth for the context-overflow phrasing, shared by:
/// - [`super::reliable`]'s non-retryable classifier (retrying the same
///   oversized request can't help),
/// - the [`super::dispatch::api_error`] Sentry-suppression cascade, and
/// - the `core::observability` `ContextWindowExceeded` classifier (which
///   catches the higher-layer re-report under `domain=agent` /
///   `web_channel`).
///
/// Status-agnostic on purpose: providers disagree on the HTTP code for this
/// condition — OpenAI / most emit `400 context_length_exceeded`, but some
/// custom / self-hosted gateways mis-report it as `500` (Sentry
/// TAURI-RUST-501: `"custom API error (500 …): Context size has been
/// exceeded."`). Matching on the body keeps all of them in one bucket.
///
/// Anchoring is deliberately two-tier because this matcher now also feeds
/// `core::observability::expected_error_kind` (Sentry suppression) and the
/// `reliable` non-retryable decision, so an over-broad match would both
/// hide a real error from Sentry *and* wrongly mark a retryable error as
/// permanent:
///
/// - **Length/context phrases** ([`CONTEXT_HINTS`]) are unambiguous —
///   "context window", "context length", "prompt is too long" only describe
///   request-size overflow — so they match alone.
/// - **Token-count phrases** ([`TOKEN_HINTS`]) collide with per-minute token
///   *rate* limits ("rate limit reached … too many tokens per min"), which
///   are transient 429s that MUST stay retryable and keep reaching Sentry.
///   They only count as context-overflow when no rate-limit marker is
///   present.
pub fn is_context_window_exceeded_message(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();

    // Unambiguous request-size / context phrases — match on their own.
    const CONTEXT_HINTS: &[&str] = &[
        "exceeds the context window",
        "context window of this model",
        "maximum context length",
        "context length exceeded",
        "context size has been exceeded",
        "prompt is too long",
        "input is too long",
        // LM Studio / llama.cpp un-evictable-prefix overflow (TAURI-RUST-6V0):
        // `"The number of tokens to keep from the initial prompt is greater
        //   than the context length (n_keep: 10978 >= n_ctx: 8192). Try to
        //   load the model with a larger context length, …"`. The user's local
        // model was loaded with an `n_ctx` smaller than the system/un-evictable
        // prefix; the remediation lives in the user's local server (reload with
        // a larger context), so this is expected user-state, not a product bug.
        "greater than the context length",
    ];
    if CONTEXT_HINTS.iter().any(|hint| lower.contains(hint)) {
        return true;
    }

    // LM Studio / llama.cpp emit the overflow as a paired `n_keep … n_ctx`
    // diagnostic. Require BOTH tokens so the arm stays anchored to that exact
    // shape (TAURI-RUST-6V0) and never broadens to unrelated `n_ctx` logging.
    if lower.contains("n_keep") && lower.contains("n_ctx") {
        return true;
    }

    // Token-count phrases are ambiguous with token-per-minute RATE limits.
    // Treat them as context-overflow only when the body carries no
    // rate-limit marker — otherwise a transient TPM 429 would be silenced
    // from Sentry and (via `reliable`) wrongly classified as non-retryable.
    const TOKEN_HINTS: &[&str] = &["too many tokens", "token limit exceeded"];
    if TOKEN_HINTS.iter().any(|hint| lower.contains(hint)) {
        const RATE_LIMIT_MARKERS: &[&str] = &[
            "per minute",
            "per min",
            "rate limit",
            "rate_limit",
            "tpm",
            "requests per",
            "retry after",
            "try again in",
        ];
        return !RATE_LIMIT_MARKERS
            .iter()
            .any(|marker| lower.contains(marker));
    }

    false
}

pub fn log_context_window_exceeded(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::warn!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "context_window_exceeded",
        "[llm_provider] {operation} context-window exceeded ({status}) — \
         request too long for the model, not reporting to Sentry"
    );
}
