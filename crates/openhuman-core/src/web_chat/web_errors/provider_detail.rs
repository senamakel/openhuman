//! Extracting and quoting the upstream provider's own error detail, and
//! naming the provider an error chain came from.

/// Pull the structured provider error message out of a raw error string.
///
/// Provider error chains from OpenAI/Anthropic/OpenRouter/etc. arrive looking
/// like `custom_openai API error (404 Not Found): {"error":{"message":"...","type":"..."}}`.
/// We extract the `error.message` value so the UI can show the *real* reason
/// — e.g. "Project ... does not have access to model `gpt-5.5`" — instead of
/// a generic apology.
///
/// Returns `None` for transport-level failures (DNS, TLS, connect refused)
/// where there is no provider body to quote — those have no actionable
/// detail and the raw error text can leak internal infrastructure URLs,
/// which the chat surface deliberately does not expose to end users.
pub(crate) fn extract_provider_error_detail(err: &str) -> Option<String> {
    const MAX_DETAIL_CHARS: usize = 300;

    // Find the first `"message"` JSON field anywhere in the error chain.
    let key = "\"message\"";
    let idx = err.find(key)?;
    let after_key = &err[idx + key.len()..];
    // Skip whitespace and the colon to the opening quote of the value.
    let after_colon = after_key.trim_start_matches(|c: char| c != '"');
    let stripped = after_colon.strip_prefix('"')?;

    // Manual unescape over the standard JSON escape set. `\uXXXX` is
    // deliberately left alone rather than decoded: this is an error-display
    // path, and an unhandled sequence should stay visible as the literal
    // `\uXXXX` instead of silently losing a character.
    let mut out = String::new();
    let mut chars = stripped.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let trimmed = out.trim();
                if trimmed.is_empty() {
                    return None;
                }
                let sanitized = crate::inference::provider::sanitize_api_error(trimmed);
                return Some(crate::util::truncate_with_ellipsis(
                    &sanitized,
                    MAX_DETAIL_CHARS,
                ));
            }
            '\\' => {
                if let Some(esc) = chars.next() {
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }

    None
}

/// Append the upstream provider detail to a user-facing message, if a useful
/// one can be extracted. Keeps the friendly summary first and the verbatim
/// provider reason below as a quotable block.
pub(crate) fn with_provider_detail(summary: &str, err: &str) -> String {
    match extract_provider_error_detail(err) {
        Some(detail) => format!("{summary}\n\n> {detail}"),
        None => summary.to_string(),
    }
}

/// Best-effort extraction of the provider name from an error string.
///
/// `inference::provider::ops::api_error` formats upstream failures as
/// `"<provider> API error (<status>): <body>"`, e.g.
/// `"openrouter API error (429 Too Many Requests): ..."`. We pull the
/// leading word and lowercase it so the wire value is stable across
/// providers' own capitalisation.
///
/// Returns `None` when:
/// - The error string doesn't carry the `" API error"` infix.
/// - The candidate word contains characters that wouldn't appear in a
///   provider name (slashes, colons, etc. — guards against transport
///   error prefixes that happen to be followed by " API error").
pub(crate) fn extract_provider_name(err: &str) -> Option<String> {
    const INFIX: &str = " API error";
    let idx = err.find(INFIX)?;
    let prefix = err[..idx].trim_end();
    let candidate = prefix
        .rsplit_once(char::is_whitespace)
        .map_or(prefix, |(_, last)| last);
    if candidate.is_empty()
        || !candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(candidate.to_ascii_lowercase())
}

/// Detect the reliable-provider aggregate that fires once every
/// configured `model_fallbacks` entry has been tried.
///
/// `reliable.rs::format_failure_aggregate` always opens with
/// `"All providers/models failed. Attempts:"`. When that marker is
/// present the FE should NOT offer a fallback retry — there is none
/// left to try.
pub(crate) fn is_fallback_chain_exhausted(err: &str) -> bool {
    err.contains("All providers/models failed")
}
