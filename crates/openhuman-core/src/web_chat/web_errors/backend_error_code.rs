//! Classifying a managed-backend error by its stable `errorCode` (#870) —
//! the trusted fast path that runs before the substring ladder in
//! [`classify_inference_error`](super::classify::classify_inference_error).

use super::classify::ClassifiedError;
use super::response_predicates::{is_malformed_tool_history_text, malformed_history_user_message};
use super::retry::{parse_retry_after_secs_from_str, retry_after_hint};

/// Classify a managed-backend error by its stable `errorCode` (#870).
///
/// Returns `Some` only when the flattened error string carries a *recognised*
/// backend `errorCode`. Because an `errorCode` is present **only** when the
/// error came through the managed backend, branching on it here lets us trust
/// the backend's verdict (operator faults route to the calm "temporarily
/// unavailable — we've been notified" copy, no user-blaming) instead of the
/// substring heuristics, which are tuned for the BYO / direct-provider path
/// (where no `errorCode` exists and "check your API key / model settings" is
/// the correct, user-actionable copy). See [`classify_inference_error`] (F2).
///
/// `None` falls through to the substring ladder, covering both the BYO path
/// (no code) and any future/unrecognised managed code we don't yet map.
pub(super) fn classify_by_backend_error_code(
    err: &str,
    provider: Option<String>,
    fallback_available: Option<bool>,
) -> Option<ClassifiedError> {
    use crate::inference::provider::{
        body_flags_malformed, extract_backend_error_code, is_managed_backend_envelope,
        BackendErrorCode,
    };

    // Managed-vs-BYO gate: an `errorCode` is only trustworthy on a
    // managed-backend envelope. A BYO / direct-provider body that merely
    // contains an `errorCode`-shaped field must fall through to the substring
    // ladder (CodeRabbit), keeping its user-actionable copy intact.
    if !is_managed_backend_envelope(err) {
        return None;
    }

    let code = extract_backend_error_code(err)?;

    // Verbose diagnostics on the new managed-code branch (per CLAUDE.md).
    // Low-cardinality only — the raw `err` may carry a provider payload / PII
    // and is logged at the caller, not here.
    log::debug!(
        "[chat-error][classify][errorCode] code={:?} provider={:?}",
        code,
        provider,
    );

    let classified = match code {
        BackendErrorCode::RateLimited => {
            let retry_secs = parse_retry_after_secs_from_str(err);
            ClassifiedError {
                error_type: "rate_limited",
                message: format!(
                    "Your AI provider is rate-limiting requests. You can retry in this thread.{}",
                    retry_after_hint(retry_secs)
                ),
                source: "provider",
                retryable: true,
                retry_after_ms: retry_secs.map(|s| s.saturating_mul(1000)),
                provider,
                fallback_available,
            }
        }
        BackendErrorCode::UserInsufficientCredits => ClassifiedError {
            error_type: "budget_exhausted",
            message: "You're out of credits. Top up, or switch to 'Use Your Own Models' \
                 in Settings."
                .to_string(),
            source: "openhuman_billing",
            retryable: false,
            retry_after_ms: None,
            provider,
            fallback_available: None,
        },
        // Operator fault (our key/account/quota/5xx) OR operator registry /
        // routing misconfig — NOT user-actionable. Both route to the same
        // calm "we've been notified" copy; the backend already paged. We
        // deliberately DROP the "check your API key" (F4) and "pick a
        // different model" (F6) copy the BYO substring arms would emit.
        BackendErrorCode::UpstreamUnavailable | BackendErrorCode::ModelUnavailable => {
            ClassifiedError {
                error_type: "provider_error",
                message: "The AI service is temporarily unavailable — we've been notified. \
                     Please try again shortly."
                    .to_string(),
                source: "provider",
                retryable: true,
                retry_after_ms: None,
                provider,
                fallback_available,
            }
        }
        BackendErrorCode::PayloadTooLarge => ClassifiedError {
            error_type: "payload_too_large",
            message: "Your message or attachment is too large for this model. Shorten it \
                 or remove the attachment — or start a new thread."
                .to_string(),
            source: "config",
            retryable: false,
            retry_after_ms: None,
            provider,
            fallback_available: None,
        },
        BackendErrorCode::ContextLengthExceeded => ClassifiedError {
            error_type: "context_overflow",
            message: "The conversation is too long. Please start a new chat.".to_string(),
            source: "config",
            retryable: false,
            retry_after_ms: None,
            provider,
            fallback_available: None,
        },
        BackendErrorCode::BadRequest => {
            // Same code, three shapes. FIRST: a tool-ordering rejection
            // (`validateToolMessageOrdering` — an orphaned `role:'tool'` message
            // with no matching assistant `tool_call`) is *poisoned history*, not
            // a model/param problem. The de-poison guard in `run_task.rs` has
            // already evicted the offending warm session by the time this copy
            // is built, so the next turn cold-boots clean — tell the user
            // exactly that (and mark retryable, because resending now works).
            if is_malformed_tool_history_text(&err.to_lowercase()) {
                ClassifiedError {
                    error_type: "provider_request_rejected",
                    message: malformed_history_user_message().to_string(),
                    source: "provider",
                    retryable: true,
                    retry_after_ms: None,
                    provider,
                    fallback_available: None,
                }
            // Else two shapes (B8/F8): a backend-flagged *malformed*
            // payload is a client bug (the request was built wrong — it pages
            // Sentry at the FE layer, gated elsewhere), while a plain
            // user-parameter rejection is a model/param mismatch the user can
            // fix. The copy differs: don't tell the user to abandon the thread
            // for a one-off malformation (only this turn failed).
            } else if body_flags_malformed(err) {
                ClassifiedError {
                    error_type: "provider_request_rejected",
                    message: "Something went wrong with this message. Try rephrasing it — \
                         or start a new thread if it keeps happening."
                        .to_string(),
                    source: "provider",
                    retryable: false,
                    retry_after_ms: None,
                    provider,
                    fallback_available: None,
                }
            } else {
                ClassifiedError {
                    error_type: "provider_request_rejected",
                    message: "The request was rejected — usually a model or parameter \
                         mismatch. Try a different model in Connections → API keys → LLM."
                        .to_string(),
                    source: "provider",
                    retryable: false,
                    retry_after_ms: None,
                    provider,
                    fallback_available: None,
                }
            }
        }
        BackendErrorCode::InternalError => ClassifiedError {
            error_type: "inference",
            // Backend already paged its own 500; the FE must not double-report
            // (gated in the Sentry classifier) and the user just retries.
            message: "Something went wrong — we've been notified. Please try again.".to_string(),
            source: "provider",
            retryable: true,
            retry_after_ms: None,
            provider,
            fallback_available,
        },
    };

    Some(classified)
}
