//! Centralised error reporting for the core, plus a Sentry
//! `before_send` filter that drops per-attempt transient-upstream
//! provider failures.
//!
//! Wraps `tracing::error!` (which the global subscriber forwards to Sentry via
//! `sentry-tracing`) inside a `sentry::with_scope` so each captured event
//! carries consistent tags identifying the failing domain/operation plus any
//! callsite-specific context (session id, request id, tool name, …).
//!
//! Why this helper exists: errors that bubble up as `Result::Err` without ever
//! being logged at error level never reach Sentry. The agent-turn path is the
//! canonical example — `run_single` used to publish a `DomainEvent::AgentError`
//! and return `Err(_)`, but Sentry never saw it. Funnel error sites through
//! `report_error` so they show up tagged and grep-friendly in Sentry.

use std::fmt::Display;

/// A `(key, value)` pair attached as a Sentry tag. Tags are short, indexed,
/// and filterable in the Sentry UI — prefer them over free-form fields for
/// anything you'd want to facet on (`error_kind`, `tool_name`, `method`).
pub type Tag<'a> = (&'a str, &'a str);

/// HTTP status codes that the reliable-provider layer already handles via
/// retry + fallback, so per-attempt Sentry reports add noise without signal:
///
/// - **408** Request Timeout
/// - **429** Too Many Requests
/// - **502** Bad Gateway
/// - **503** Service Unavailable
/// - **504** Gateway Timeout
///
/// Single source of truth for both the call-site classifier
/// (`openhuman::providers::ops::should_report_provider_http_failure`) and the
/// `before_send` filter (`is_transient_provider_http_failure`). Update here
/// and both sites pick it up — keeps the two layers from drifting.
pub const TRANSIENT_PROVIDER_HTTP_STATUSES: &[u16] = &[408, 429, 502, 503, 504, 520];

/// HTTP status codes that represent transient backend / integration transport
/// failures rather than application bugs. Keep this as strings because Sentry
/// tags are strings, and the before_send classifiers match tag values exactly.
pub const TRANSIENT_HTTP_STATUSES: &[&str] = &["408", "429", "502", "503", "504", "520"];

/// Transport-layer phrases observed from reqwest / hyper for temporary
/// upstream interruptions. Keep these specific so rare configuration failures
/// still reach Sentry.
pub const TRANSIENT_TRANSPORT_PHRASES: &[&str] = &[
    "timeout",
    "operation timed out",
    "connection forcibly closed",
    "connection reset",
    "tls handshake eof",
    "error sending request",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedErrorKind {
    LocalAiDisabled,
    ApiKeyMissing,
}

pub fn expected_error_kind(message: &str) -> Option<ExpectedErrorKind> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("local ai is disabled") {
        return Some(ExpectedErrorKind::LocalAiDisabled);
    }
    if lower.contains("api key not set") || lower.contains("missing api key") {
        return Some(ExpectedErrorKind::ApiKeyMissing);
    }
    None
}

/// Capture an error to Sentry with structured tags.
///
/// `domain` and `operation` are required and become tags `domain:<…>` and
/// `operation:<…>`. `extra` is an optional list of extra tag pairs. The error
/// itself is rendered via `Display` and emitted as a `tracing::error!` event,
/// which the Sentry tracing layer turns into a Sentry event under the active
/// scope.
///
/// Use stable, low-cardinality values for tag keys/values so Sentry can group
/// and aggregate. High-cardinality data (full IDs, payloads) belongs in the
/// error message body, not in tags.
pub fn report_error<E: Display + ?Sized>(
    err: &E,
    domain: &str,
    operation: &str,
    extra: &[Tag<'_>],
) {
    // Use the alternate format specifier so `anyhow::Error` renders its full
    // context chain (outer context + every wrapped cause, joined by ": ").
    // Plain `Display` impls fall back to the standard representation. Without
    // this, anyhow's default `to_string()` only emits the outermost context
    // and the underlying cause (e.g. a `toml::de::Error` with line/column) is
    // dropped — making the captured Sentry event undiagnosable. See
    // OPENHUMAN-TAURI-B2 for an instance where this masked the real failure.
    let message = format!("{err:#}");
    report_error_message(&message, domain, operation, extra);
}

/// Report an error unless it is an expected user-state/config condition.
///
/// Expected conditions are logged at `info` or `warn` so the Sentry tracing
/// layer records at most a breadcrumb, not an error event.
pub fn report_error_or_expected<E: Display + ?Sized>(
    err: &E,
    domain: &str,
    operation: &str,
    extra: &[Tag<'_>],
) {
    let message = format!("{err:#}");
    if let Some(kind) = expected_error_kind(&message) {
        report_expected_message(kind, &message, domain, operation);
        return;
    }
    report_error_message(&message, domain, operation, extra);
}

fn report_expected_message(kind: ExpectedErrorKind, message: &str, domain: &str, operation: &str) {
    match kind {
        ExpectedErrorKind::LocalAiDisabled => {
            tracing::info!(
                domain = domain,
                operation = operation,
                error = %message,
                "[observability] {domain}.{operation} skipped expected local-ai disabled error: {message}"
            );
        }
        ExpectedErrorKind::ApiKeyMissing => {
            tracing::warn!(
                domain = domain,
                operation = operation,
                error = %message,
                "[observability] {domain}.{operation} skipped expected API-key configuration error: {message}"
            );
        }
    }
}

/// Distinct `tracing::Metadata::target()` we set on the diagnostic
/// `tracing::error!` emitted from [`report_error_message`].
///
/// Sentry capture for this helper happens via an explicit
/// `sentry::capture_message` call below — not via the `sentry-tracing`
/// layer scooping up the `tracing::error!` event. The production
/// `sentry_tracing_layer()` in `core::logging` filters events with this
/// target to `EventFilter::Ignore` so we never double-report (one direct
/// `capture_message`, one tracing-bridge capture of the same condition).
///
/// Why direct capture instead of relying on the bridge: the bridge worked
/// in steady-state but flaked under parallel test scheduling
/// (`thread_not_found_rpc_error_does_not_report_to_sentry` repeatedly hit
/// `events.len() == 0` in CI even with a thread-default subscriber wired
/// up — likely a Linux-only thread-local ordering quirk in
/// `sentry-tracing`'s `Hub::current()` lookup at event-emit time). Direct
/// `sentry::capture_message` synchronously routes through the active hub
/// and is deterministic, which keeps both production reporting and tests
/// honest.
pub const REPORT_ERROR_TRACING_TARGET: &str = "openhuman::observability::report_error";

pub(crate) fn report_error_message(
    message: &str,
    domain: &str,
    operation: &str,
    extra: &[Tag<'_>],
) {
    sentry::with_scope(
        |scope| {
            scope.set_tag("domain", domain);
            scope.set_tag("operation", operation);
            for (k, v) in extra {
                scope.set_tag(k, v);
            }
        },
        || {
            // Direct, synchronous Sentry capture — see
            // `REPORT_ERROR_TRACING_TARGET` for why we don't rely on the
            // `sentry-tracing` layer for this call site.
            sentry::capture_message(message, sentry::Level::Error);
            // Diagnostic log line for stderr / file appenders. Tagged with
            // the marker target so the production sentry-tracing layer
            // skips it (no double Sentry event).
            tracing::error!(
                target: REPORT_ERROR_TRACING_TARGET,
                domain = domain,
                operation = operation,
                error = %message,
                "[observability] {domain}.{operation} failed: {message}"
            );
        },
    );
}

/// Returns true when a Sentry event is a per-attempt provider HTTP failure
/// that the reliable-provider layer already handles via retry + fallback.
///
/// The primary suppression lives at the call site
/// (`openhuman::providers::ops::should_report_provider_http_failure`),
/// which short-circuits transient codes before `report_error` ever fires.
/// This helper is intended for use inside the `sentry::ClientOptions`
/// `before_send` hook as defense-in-depth — it catches any future call
/// site that emits a `tracing::error!` with the same shape but bypasses
/// the classifier.
///
/// Match criteria (all required):
/// - tag `domain == "llm_provider"` — pins the filter to provider-originated
///   events so an unrelated subsystem emitting `failure=non_2xx`/`status=503`
///   for its own reasons doesn't get silently dropped
/// - tag `failure == "non_2xx"` (the marker set by `ops::api_error`)
/// - tag `status` parses to one of [`TRANSIENT_PROVIDER_HTTP_STATUSES`]
pub fn is_transient_provider_http_failure(event: &sentry::protocol::Event<'_>) -> bool {
    let tags = &event.tags;
    if tags.get("domain").map(String::as_str) != Some("llm_provider") {
        return false;
    }
    if tags.get("failure").map(String::as_str) != Some("non_2xx") {
        return false;
    }
    let Some(status_u16) = tags.get("status").and_then(|s| s.parse::<u16>().ok()) else {
        return false;
    };
    TRANSIENT_PROVIDER_HTTP_STATUSES.contains(&status_u16)
}

pub fn is_transient_http_status(status: &str) -> bool {
    TRANSIENT_HTTP_STATUSES.contains(&status)
}

pub fn is_transient_http_status_code(status: u16) -> bool {
    let status = status.to_string();
    is_transient_http_status(status.as_str())
}

pub fn contains_transient_transport_phrase(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    TRANSIENT_TRANSPORT_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

fn event_has_transient_transport_phrase(event: &sentry::protocol::Event<'_>) -> bool {
    event
        .message
        .as_deref()
        .is_some_and(contains_transient_transport_phrase)
        || event
            .logentry
            .as_ref()
            .is_some_and(|log| contains_transient_transport_phrase(&log.message))
        || event.exception.values.iter().any(|exception| {
            exception
                .value
                .as_deref()
                .is_some_and(contains_transient_transport_phrase)
        })
}

fn is_transient_domain_failure(event: &sentry::protocol::Event<'_>, domain: &str) -> bool {
    let tags = &event.tags;
    if tags.get("domain").map(String::as_str) != Some(domain) {
        return false;
    }

    match tags.get("failure").map(String::as_str) {
        Some("non_2xx") => tags
            .get("status")
            .is_some_and(|status| is_transient_http_status(status)),
        Some("transport") => event_has_transient_transport_phrase(event),
        _ => false,
    }
}

/// Transient backend API failures (gateway hiccups, scheduled downtime).
/// Match by event tags written by report_error at the authed_json call site.
pub fn is_transient_backend_api_failure(event: &sentry::protocol::Event<'_>) -> bool {
    is_transient_domain_failure(event, "backend_api")
}

/// Transient integrations / Composio failures (timeout, connection reset,
/// gateway hiccups).
pub fn is_transient_integrations_failure(event: &sentry::protocol::Event<'_>) -> bool {
    is_transient_domain_failure(event, "integrations")
}

/// String tokens that mark a formatted error message as a transient HTTP
/// failure. Used at upstream emit sites (`rpc.invoke_method`,
/// `web_channel.run_chat_task`) where the error has already been stringified
/// and the original `status` / `failure` tag context is gone.
///
/// Each token combines a status code with a non-numeric anchor (parenthesis
/// or canonical reason phrase) so bare numeric coincidences ("process 502
/// exited") do not match.
const TRANSIENT_STATUS_MESSAGE_TOKENS: &[&str] = &[
    "(408 ",
    "(429 ",
    "(502 ",
    "(503 ",
    "(504 ",
    "(520 ",
    "408 request timeout",
    "429 too many requests",
    "502 bad gateway",
    "503 service unavailable",
    "504 gateway timeout",
    "520 <unknown status code>",
];

/// Returns true when a formatted error message describes a transient HTTP
/// or transport-layer failure that has already been demoted further down the
/// stack. Use at upstream re-emit sites (`rpc.invoke_method`,
/// `web_channel.run_chat_task`) where `report_error` is called with the
/// stringified downstream error and no `failure` / `status` tag context.
pub fn is_transient_message_failure(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    TRANSIENT_STATUS_MESSAGE_TOKENS
        .iter()
        .any(|token| lower.contains(token))
        || contains_transient_transport_phrase(&lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper must accept `&anyhow::Error`, `&dyn std::error::Error`, and
    /// plain `&str` — the three shapes that show up at error sites today.
    #[test]
    fn report_error_accepts_common_error_shapes() {
        let anyhow_err = anyhow::anyhow!("boom");
        report_error(&anyhow_err, "test", "anyhow_shape", &[]);

        let io_err = std::io::Error::other("io failed");
        report_error(&io_err, "test", "io_shape", &[("kind", "io")]);

        report_error("plain message", "test", "str_shape", &[]);
    }

    #[test]
    fn anyhow_chain_is_rendered_in_full() {
        // Regression guard: `err.to_string()` on an anyhow chain only emits
        // the outermost context. Using `{:#}` joins every cause, which is
        // what Sentry needs to actually diagnose wrapped failures.
        let inner = std::io::Error::other("inner cause");
        let wrapped = anyhow::Error::from(inner).context("outer ctx");
        assert_eq!(format!("{wrapped:#}"), "outer ctx: inner cause");
    }

    #[test]
    fn classifies_expected_config_errors() {
        assert_eq!(
            expected_error_kind("rpc.invoke_method failed: local ai is disabled"),
            Some(ExpectedErrorKind::LocalAiDisabled)
        );
        assert_eq!(
            expected_error_kind(
                "agent.provider_chat failed: ollama API key not set. Configure via the web UI"
            ),
            Some(ExpectedErrorKind::ApiKeyMissing)
        );
        assert_eq!(
            expected_error_kind("ollama embed failed with status 500"),
            None
        );
    }

    #[test]
    fn report_error_does_not_panic_with_many_tags() {
        let err = anyhow::anyhow!("multi-tag");
        report_error(
            &err,
            "test",
            "multi_tag",
            &[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")],
        );
    }

    fn event_with_tags(pairs: &[(&str, &str)]) -> sentry::protocol::Event<'static> {
        let mut event = sentry::protocol::Event::default();
        let mut tags: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for (k, v) in pairs {
            tags.insert((*k).to_string(), (*v).to_string());
        }
        event.tags = tags;
        event
    }

    fn event_with_tags_and_message(
        pairs: &[(&str, &str)],
        message: &str,
    ) -> sentry::protocol::Event<'static> {
        let mut event = event_with_tags(pairs);
        event.message = Some(message.to_string());
        event
    }

    #[test]
    fn transient_filter_drops_429_408_502_503_504() {
        for status in ["429", "408", "502", "503", "504"] {
            let event = event_with_tags(&[
                ("domain", "llm_provider"),
                ("failure", "non_2xx"),
                ("status", status),
            ]);
            assert!(
                is_transient_provider_http_failure(&event),
                "status {status} must be classified as transient and filtered"
            );
        }
    }

    #[test]
    fn transient_filter_keeps_permanent_failures() {
        for status in ["400", "401", "403", "404", "500"] {
            let event = event_with_tags(&[
                ("domain", "llm_provider"),
                ("failure", "non_2xx"),
                ("status", status),
            ]);
            assert!(
                !is_transient_provider_http_failure(&event),
                "status {status} must NOT be filtered — it's actionable"
            );
        }
    }

    #[test]
    fn transient_filter_keeps_aggregate_all_exhausted() {
        let event = event_with_tags(&[
            ("domain", "llm_provider"),
            ("failure", "all_exhausted"),
            ("status", "503"),
        ]);
        assert!(
            !is_transient_provider_http_failure(&event),
            "aggregate all_exhausted events must surface (they are the cascade signal)"
        );
    }

    #[test]
    fn transient_filter_keeps_events_with_no_status_tag() {
        let event = event_with_tags(&[("domain", "llm_provider"), ("failure", "non_2xx")]);
        assert!(
            !is_transient_provider_http_failure(&event),
            "missing status tag must not be silently dropped"
        );
    }

    // Regression guard: the filter must scope to provider events only. Other
    // subsystems emit `failure=non_2xx` (e.g.
    // `providers/compatible.rs` uses the same marker for OAI-compatible
    // error paths, but every site goes through `report_error(..,
    // "llm_provider", ..)` so the domain tag is consistent), but the broader
    // point is: any future caller that re-uses the same tag set for a
    // different domain must NOT be silently dropped by this filter.
    #[test]
    fn transient_filter_keeps_events_with_no_domain_tag() {
        let event = event_with_tags(&[("failure", "non_2xx"), ("status", "503")]);
        assert!(
            !is_transient_provider_http_failure(&event),
            "missing domain tag means the event isn't provider-originated — must surface"
        );
    }

    #[test]
    fn transient_filter_keeps_events_from_other_domains() {
        let event = event_with_tags(&[
            ("domain", "scheduler"),
            ("failure", "non_2xx"),
            ("status", "503"),
        ]);
        assert!(
            !is_transient_provider_http_failure(&event),
            "non-provider domain must surface even if failure/status tags collide"
        );
    }

    #[test]
    fn backend_api_filter_drops_transient_statuses() {
        for status in TRANSIENT_HTTP_STATUSES {
            let event = event_with_tags(&[
                ("domain", "backend_api"),
                ("failure", "non_2xx"),
                ("status", status),
            ]);
            assert!(
                is_transient_backend_api_failure(&event),
                "backend status {status} must be classified as transient"
            );
        }
    }

    #[test]
    fn backend_api_filter_drops_transient_transport_phrases() {
        for phrase in TRANSIENT_TRANSPORT_PHRASES {
            let event = event_with_tags_and_message(
                &[("domain", "backend_api"), ("failure", "transport")],
                &format!("GET /teams failed: {phrase}"),
            );
            assert!(
                is_transient_backend_api_failure(&event),
                "backend transport phrase {phrase} must be classified as transient"
            );
        }
    }

    #[test]
    fn backend_api_filter_keeps_non_transient_failures() {
        for status in ["404", "500"] {
            let event = event_with_tags(&[
                ("domain", "backend_api"),
                ("failure", "non_2xx"),
                ("status", status),
            ]);
            assert!(
                !is_transient_backend_api_failure(&event),
                "backend status {status} must stay visible"
            );
        }

        let wrong_domain = event_with_tags(&[
            ("domain", "scheduler"),
            ("failure", "non_2xx"),
            ("status", "503"),
        ]);
        assert!(
            !is_transient_backend_api_failure(&wrong_domain),
            "domain scoping must keep unrelated transient-shaped events visible"
        );

        let non_matching_transport = event_with_tags_and_message(
            &[("domain", "backend_api"), ("failure", "transport")],
            "GET /teams failed: certificate verify failed",
        );
        assert!(
            !is_transient_backend_api_failure(&non_matching_transport),
            "transport failures without an allowlisted phrase must stay visible"
        );
    }

    #[test]
    fn integrations_filter_drops_transient_statuses() {
        for status in TRANSIENT_HTTP_STATUSES {
            let event = event_with_tags(&[
                ("domain", "integrations"),
                ("failure", "non_2xx"),
                ("status", status),
            ]);
            assert!(
                is_transient_integrations_failure(&event),
                "integrations status {status} must be classified as transient"
            );
        }
    }

    #[test]
    fn integrations_filter_drops_transient_transport_phrases() {
        for phrase in TRANSIENT_TRANSPORT_PHRASES {
            let event = event_with_tags_and_message(
                &[("domain", "integrations"), ("failure", "transport")],
                &format!("GET /agent-integrations/tools failed: {phrase}"),
            );
            assert!(
                is_transient_integrations_failure(&event),
                "integrations transport phrase {phrase} must be classified as transient"
            );
        }
    }

    #[test]
    fn integrations_filter_keeps_non_transient_failures() {
        for status in ["404", "500"] {
            let event = event_with_tags(&[
                ("domain", "integrations"),
                ("failure", "non_2xx"),
                ("status", status),
            ]);
            assert!(
                !is_transient_integrations_failure(&event),
                "integrations status {status} must stay visible"
            );
        }

        let wrong_domain = event_with_tags(&[
            ("domain", "composio"),
            ("failure", "non_2xx"),
            ("status", "503"),
        ]);
        assert!(
            !is_transient_integrations_failure(&wrong_domain),
            "domain scoping must keep composio-tagged events visible"
        );

        let non_matching_transport = event_with_tags_and_message(
            &[("domain", "integrations"), ("failure", "transport")],
            "GET /agent-integrations/tools failed: invalid certificate",
        );
        assert!(
            !is_transient_integrations_failure(&non_matching_transport),
            "transport failures without an allowlisted phrase must stay visible"
        );
    }

    #[test]
    fn message_failure_classifier_matches_canonical_status_phrases() {
        for msg in [
            "rpc.invoke_method failed: GET /teams failed (502 Bad Gateway)",
            "GET /teams/me/usage failed (503 Service Unavailable)",
            "downstream returned (504 Gateway Timeout): retry budget exhausted",
            "OpenHuman API error (520 <unknown status code>): cf",
            "POST /channels/telegram/typing failed (429 Too Many Requests)",
            "auth connect failed: 503 Service Unavailable",
        ] {
            assert!(
                is_transient_message_failure(msg),
                "{msg:?} must be classified as transient"
            );
        }
    }

    #[test]
    fn message_failure_classifier_matches_transport_phrases() {
        for msg in [
            "integrations.get failed: composio/tools → operation timed out",
            "GET https://api.example.com → connection forcibly closed (os 10054)",
            "POST /v1/foo → tls handshake eof",
            "error sending request for url (https://api.example.com)",
        ] {
            assert!(
                is_transient_message_failure(msg),
                "{msg:?} must be classified as transient"
            );
        }
    }

    #[test]
    fn message_failure_classifier_keeps_unrelated_messages() {
        for msg in [
            "rpc.invoke_method failed: schema validation error",
            "process 502 exited unexpectedly",
            "GET /teams failed (404 Not Found)",
            "GET /teams failed (500 Internal Server Error)",
            "unrelated error with port 5023",
            "",
        ] {
            assert!(
                !is_transient_message_failure(msg),
                "{msg:?} must not be classified as transient"
            );
        }
    }

    #[test]
    fn report_error_or_expected_does_not_panic() {
        report_error_or_expected(
            "local ai is disabled",
            "rpc",
            "invoke_method",
            &[("method", "openhuman.local_ai_prompt")],
        );
        report_error_or_expected(
            "ollama API key not set",
            "agent",
            "provider_chat",
            &[("provider", "ollama")],
        );
    }
}
