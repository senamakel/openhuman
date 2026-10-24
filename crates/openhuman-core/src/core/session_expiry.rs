//! Which controller errors mean the OpenHuman session has expired.
//!
//! [`invoke_method`](super::invoke::invoke_method) publishes `SessionExpired`
//! when [`is_session_expired_error`] matches, and the JSON-RPC transport in
//! `openhuman-rpc` uses the same predicate to keep those failures out of
//! Sentry.

/// Helper to determine if an error message indicates an expired or invalid
/// OpenHuman backend session.
///
/// **Narrower than the previous implementation** (fixed in issue #2286):
///
/// The old predicate matched ANY `"401 + unauthorized"` pattern, which caused
/// downstream provider 401s (Discord bot token failures, BYO-key OpenAI /
/// Anthropic failures, Composio direct-mode errors) to clear the user's session
/// and log them out. The fix distinguishes between:
///
/// - **OpenHuman backend 401s** (`authed_json` in `crates/openhuman-core/src/api/rest.rs`): formatted
///   as `"{METHOD} /path failed (401 Unauthorized): {body}"`, e.g.
///   `"GET /teams failed (401 Unauthorized): {"success":false}"`. These always
///   start with an HTTP method verb followed by a space and a forward slash.
/// - **Provider / downstream 401s** (`api_error` in
///   `crates/openhuman-core/src/inference/provider/http_error.rs`): formatted as
///   `"{ProviderName} API error (401 Unauthorized): {body}"` or
///   `"Discord API error: ... (401): Unauthorized"`. These start with a
///   provider name, NOT an HTTP method verb.
///
/// **What still triggers session expiry:**
/// - `"Session expired"` — explicit body text from the OpenHuman backend.
/// - `"no backend session token"` — pre-flight guard; auth profile is missing.
/// - `"session jwt required"` — local guard; JWT already cleared by a prior 401.
/// - `"SESSION_EXPIRED"` — scheduler-gate sentinel (exact case).
/// - HTTP-method-prefixed 401s (`GET /`, `POST /`, etc.) — backend path format.
///
/// **What no longer triggers session expiry (fixed in #2286):**
/// - Provider-prefixed 401s (`"Discord API error: ..."`, `"OpenAI API error ..."`)
/// - `"invalid token"` — too broad; also matches Discord / OAuth provider tokens.
///
/// Note: for inference-path OpenHuman backend 401s, `api_error` (in
/// `inference/provider/http_error.rs`) ALREADY publishes `SessionExpired`
/// directly, so there is no regression if this predicate misses them — the
/// subscriber is idempotent and a harmless double-publish would still be correct.
pub fn is_session_expired_error(msg: &str) -> bool {
    // Explicit session-expired markers from the OpenHuman backend / local
    // guards — delegated to the shared observability classifier so both the
    // Sentry expected-error pipeline and the JSON-RPC publish boundary stay
    // in lock-step.
    if crate::core::observability::is_session_expired_message(msg) {
        return true;
    }
    // OpenHuman backend path 401s via `authed_json`:
    // format is "{METHOD} /path failed (401 Unauthorized): {body}"
    // The HTTP-method prefix distinguishes these from provider-prefixed errors.
    // HEAD and OPTIONS are intentionally excluded — `authed_json` only issues
    // the five listed verbs (GET/POST/PUT/DELETE/PATCH) for REST JSON endpoints.
    let lower = msg.to_ascii_lowercase();
    if (lower.contains("401") && lower.contains("unauthorized"))
        && (msg.starts_with("GET /")
            || msg.starts_with("POST /")
            || msg.starts_with("PUT /")
            || msg.starts_with("DELETE /")
            || msg.starts_with("PATCH /"))
    {
        return true;
    }
    false
}

/// Detect auth-looking failures that are not specific enough to clear the
/// OpenHuman session. This is only for diagnostics; it must not feed the
/// `SessionExpired` publish path.
///
/// Matches a generic `401 Unauthorized` OR a bare `"invalid token"` string,
/// either of which can come from BYO-key providers, Composio, channels, or
/// other scoped downstream calls. Used exclusively for diagnostic logging
/// at the `invoke_method` call site so provider auth failures are visible
/// in the logs without being misclassified as session expiry.
pub fn is_unconfirmed_unauthorized_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    (lower.contains("401") && lower.contains("unauthorized")) || lower.contains("invalid token")
}

#[cfg(test)]
#[path = "session_expiry_tests.rs"]
mod tests;
