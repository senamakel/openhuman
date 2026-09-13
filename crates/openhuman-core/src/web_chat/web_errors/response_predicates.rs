//! String-flat predicates over a lowercased error message: recognising the
//! shapes of a poisoned tool-call history, an empty provider response, a
//! dropped connection, a rejected 4xx, or a transient outage.

/// String-flat mirror of
/// `crate::core::observability::is_empty_provider_response_message`.
///
/// The typed `AgentError::EmptyProviderResponse` is collapsed to a `String`
/// at the native-bus boundary before reaching this layer, so we re-detect
/// the same canonical phrase the agent harness emits. Anchored on
/// `"model returned an empty response"` (the verbatim user-facing string from
/// `AgentError::EmptyProviderResponse`) — NOT the looser `"empty response"`,
/// so internal fall-through phrases (`"summarizer returned empty response"`,
/// `"provider returned an empty response; returning empty extraction"`) are
/// not misclassified. Keep the anchor in lockstep with the observability
/// mirror.
///
/// Caller passes the already-lowercased error string.
pub(crate) fn is_empty_provider_response_text(lower: &str) -> bool {
    lower.contains("model returned an empty response")
}

/// User-facing copy for a poisoned-history 400 (orphaned tool message). The
/// de-poison guard (`run_task.rs`) has already evicted the offending warm
/// session by the time this is shown, so "send it again" is literally true.
pub(crate) fn malformed_history_user_message() -> &'static str {
    "We hit a temporary glitch in this conversation — we've cleared it. \
     Please send your message again."
}

/// Detect a malformed tool-history rejection (orphaned / mismatched
/// `role:'tool'` message). This is the *poisoned history* shape the de-poison
/// guard recovers from — NOT a model/parameter mismatch — so it earns the
/// "we cleared it, resend" copy instead of "try a different model".
///
/// Anchored on the managed backend's `validateToolMessageOrdering` strings
/// (verified against tinyhumansai/backend `chatCompletions.ts` — "role 'tool' …
/// matching tool_call", "does not match any tool_call from the preceding
/// assistant message"), the raw upstream jinja variant ("tool role … no
/// previous assistant message with a tool call"), and the equivalent BYO
/// provider phrasings. Caller passes the already-lowercased error string.
pub(crate) fn is_malformed_tool_history_text(lower: &str) -> bool {
    let tool_role = lower.contains("role 'tool'") || lower.contains("tool role");
    let about_tool_call = lower.contains("tool call") || lower.contains("tool_call");
    (tool_role && about_tool_call)
        || lower.contains("does not match any tool_call from the preceding assistant message")
}

/// Detect a transport-level connection drop with no provider status / managed
/// `errorCode` — the residue that otherwise falls to the generic `inference`
/// catch-all (issue #3714 bucket #1).
///
/// Anchored on the canonical reqwest/hyper shapes for a severed or never-opened
/// connection (stale keep-alive reused after sleep/wake, network change, raw
/// mid-stream SSE drop). Intentionally does NOT match `"timed out"` (the
/// dedicated `timeout` arm owns that) nor any `4xx/5xx` status (those arms claim
/// their shapes earlier). Caller passes the already-lowercased error string.
pub(crate) fn is_connection_dropped_text(lower: &str) -> bool {
    const DROP_MARKERS: &[&str] = &[
        "connection closed before message completed", // hyper IncompleteMessage
        "error reading a body from connection",
        "connection reset",
        "connection refused",
        "connection aborted",
        "broken pipe",
        "unexpected end of file",
        "unexpected eof",
        "error sending request",
        "tcp connect error",
        "dns error",
        "failed to lookup address",
    ];
    DROP_MARKERS.iter().any(|marker| lower.contains(marker))
}

/// Detect an un-claimed provider 4xx (generic client-side request rejection).
///
/// Mirrors the status tokens emitted by `inference::provider::ops::api_error`
/// (`"<provider> API error (400 Bad Request): …"`). Ordered AFTER the
/// provider-config-rejection and model-unavailable arms in
/// [`classify_inference_error`](super::classify::classify_inference_error), so
/// only 4xx shapes those arms did not claim reach this predicate.
///
/// Caller passes the already-lowercased error string.
pub(crate) fn is_provider_request_rejected_text(lower: &str) -> bool {
    // Match only when the 4xx status appears inside a provider error envelope
    // (`<provider> API error (4xx …)`, emitted by
    // `inference::provider::ops::api_error`). Matching a bare "400"/"404"
    // anywhere would misclassify unrelated errors that merely contain those
    // digits (token counts, byte offsets, timestamps). Per CodeRabbit review
    // on PR #3199.
    const PROVIDER_4XX_MARKERS: &[&str] = &[
        "api error (400",
        "api error (404",
        "api error (409",
        "api error (422",
    ];
    PROVIDER_4XX_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Whether a model-availability error body describes a **transient** upstream
/// outage rather than a user misconfiguration (#5503).
///
/// The `model_unavailable` arm matches on the bare word `unavailable`, which a
/// provider emits for BOTH "you picked a model I don't host" (config, terminal)
/// and "this model is temporarily down / overloaded right now" (transient,
/// retryable). Only the second class carries one of these temporary-outage
/// markers, so it's the safe discriminator: a terminal endpoint rejection like
/// `"model unavailable on this endpoint"` (a 404 for a model that endpoint
/// doesn't host) carries none of them and stays on the config verdict.
///
/// Deliberately does NOT key on the bare word `unavailable` — that's the very
/// ambiguity being disambiguated. Caller passes the already-lowercased string.
pub(crate) fn is_transient_unavailability_text(lower: &str) -> bool {
    const TRANSIENT_MARKERS: &[&str] = &[
        "temporarily",
        "temporary",
        "currently unavailable",
        "currently overloaded",
        "overloaded",
        "try again later",
        "try again in a",
        "please retry",
    ];
    TRANSIENT_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}
