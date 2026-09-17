//! Shared secret-scrubbing for anything written to stderr / file logs.
//!
//! Diagnostic log lines (e.g. `core::observability::report_error_message`) can
//! carry error strings that embed bearer tokens, API keys, or other secrets. In
//! slim builds compiled without `crash-reporting` there is no Sentry
//! `before_send` hook to sanitise them, and even in full builds the
//! `before_send` hook only scrubs the *Sentry event* — not the parallel
//! `tracing` log line. This module owns the one redaction pass used by both the
//! Sentry path (`src/main.rs`) and the always-on log path, so the patterns
//! cannot drift between them. Always compiled (no feature gate).

use once_cell::sync::Lazy;
use regex::Regex;

static SECRET_PATTERNS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    vec![
        // Matches "Bearer <token>" and redacts the token.
        (Regex::new(r"(?i)(bearer\s+)\S+").unwrap(), "${1}[REDACTED]"),
        // Matches "api-key: <key>" or "api_key=<key>" and redacts the key.
        (
            Regex::new(r"(?i)(api[_-]?key[=:\s]+)\S+").unwrap(),
            "${1}[REDACTED]",
        ),
        // \b anchor prevents matching `cancellation_token=` etc.
        (
            Regex::new(r"(?i)\b(token[=:\s]+)\S+").unwrap(),
            "${1}[REDACTED]",
        ),
        // Anthropic keys (sk-ant-api03-...) contain hyphens the generic
        // sk- pattern below won't match.
        (
            Regex::new(r"sk-ant-[A-Za-z0-9\-_]{16,}").unwrap(),
            "[REDACTED]",
        ),
        // OpenAI admin keys (sk-admin-...).
        (
            Regex::new(r"sk-admin-[A-Za-z0-9\-_]{12,}").unwrap(),
            "[REDACTED]",
        ),
        // OpenAI project-scoped and org-scoped keys (sk-proj-... / sk-org-...).
        (
            Regex::new(r"sk-(?:proj|org)-[A-Za-z0-9\-_]{12,}").unwrap(),
            "[REDACTED]",
        ),
        // Generic catch-all for any sk- format not covered above. Includes `-`
        // and `_` in the suffix so a separator mid-token can't leave a trailing
        // fragment unredacted (e.g. `sk-…_uv` → `[REDACTED]_uv`).
        (Regex::new(r"sk-[A-Za-z0-9_-]{20,}").unwrap(), "[REDACTED]"),
    ]
});

/// Replace substrings that look like secrets with `[REDACTED]`.
///
/// Intended for anything about to be written to a log sink or an error report;
/// it redacts the secret-looking span in place and leaves the rest of the
/// diagnostic message intact (unlike a whole-value prefix redaction).
pub fn scrub_secrets(input: &str) -> String {
    let mut result = input.to_string();
    for (re, replacement) in SECRET_PATTERNS.iter() {
        result = re.replace_all(&result, *replacement).into_owned();
    }
    result
}

#[cfg(test)]
#[path = "log_redaction_tests.rs"]
mod tests;
