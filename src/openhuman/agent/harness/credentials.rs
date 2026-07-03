use regex::Regex;
use std::sync::LazyLock;

/// Key/value secrets: `token=…`, `api_key: "…"`, `password='…'`, etc. Matches a
/// known credential key followed by `:`/`=` and a value of ≥8 chars (quoted or
/// bare). This is the legacy pattern the in-house engine ran on every tool
/// output.
static SENSITIVE_KV_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(token|api[_-]?key|password|secret|user[_-]?key|bearer|credential)["']?\s*[:=]\s*(?:"([^"]{8,})"|'([^']{8,})'|([a-zA-Z0-9_\-\.]{8,}))"#).unwrap()
});

/// Standalone AWS access-key IDs (`AKIA…`, `ASIA…`) — a fixed 20-char token that
/// carries no `key: value` framing, so the KV regex above never catches a bare
/// occurrence (env dumps, JSON API responses, shell output).
static AWS_ACCESS_KEY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b((?:AKIA|ASIA)[0-9A-Z]{16})\b").unwrap());

/// Standalone OpenAI-style secret keys (`sk-…`, `sk-proj-…`) that appear without
/// a preceding credential key.
static OPENAI_KEY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(sk-[A-Za-z0-9_\-]{20,})\b").unwrap());

/// `Bearer <token>` authorization values (space-separated, so the KV regex —
/// which needs a `:`/`=` immediately after the keyword — misses them).
static BEARER_TOKEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(bearer)\s+([A-Za-z0-9._\-]{8,})").unwrap());

/// Redact a raw secret value, preserving up to the first 4 chars for context.
/// UTF-8 safe (slices on a char boundary). Values ≤4 chars are fully redacted.
fn redact_value(val: &str) -> String {
    let prefix = if val.chars().count() > 4 {
        match val.char_indices().nth(4) {
            Some((idx, _)) => &val[..idx],
            None => val,
        }
    } else {
        ""
    };
    format!("{prefix}*[REDACTED]")
}

/// Scrub credentials from tool output to prevent accidental exfiltration.
///
/// Replaces known credential patterns with a redacted placeholder while
/// preserving a small prefix for context. Runs the key/value pattern first, then
/// standalone well-known secret formats (AWS access keys, OpenAI `sk-` keys,
/// `Bearer` tokens) that carry no `key: value` framing. Idempotent: re-scrubbing
/// already-redacted text is a no-op (the redacted prefix is ≤4 chars and no
/// pattern re-matches `*[REDACTED]`).
///
/// This is the single source of truth for credential scrubbing on the agent
/// path — see the `CredentialScrubMiddleware` seam
/// (`src/openhuman/tinyagents/middleware.rs`) which runs it over every raw tool
/// output before summarization/caps/persistence.
pub(crate) fn scrub_credentials(input: &str) -> String {
    // 1. Key/value secrets (legacy engine behaviour). The reconstructed output
    //    preserves the original delimiter/quoting so the model still sees a
    //    well-formed line.
    let kv_scrubbed = SENSITIVE_KV_REGEX.replace_all(input, |caps: &regex::Captures| {
        let full_match = &caps[0];
        let key = &caps[1];
        let val = caps
            .get(2)
            .or(caps.get(3))
            .or(caps.get(4))
            .map(|m| m.as_str())
            .unwrap_or("");

        // Preserve first 4 chars for context, then redact.
        let prefix = if val.chars().count() > 4 {
            match val.char_indices().nth(4) {
                Some((idx, _)) => &val[..idx],
                None => val,
            }
        } else {
            ""
        };

        if full_match.contains(':') {
            if full_match.contains('"') {
                format!("\"{key}\": \"{prefix}*[REDACTED]\"")
            } else {
                format!("{key}: {prefix}*[REDACTED]")
            }
        } else if full_match.contains('=') {
            if full_match.contains('"') {
                format!("{key}=\"{prefix}*[REDACTED]\"")
            } else {
                format!("{key}={prefix}*[REDACTED]")
            }
        } else {
            format!("{key}: {prefix}*[REDACTED]")
        }
    });

    // 2. `Bearer <token>` — redact the token, keep the scheme keyword so the
    //    shape (`Authorization: Bearer …`) survives for the model. Runs before
    //    the bare-token passes so a `Bearer AKIA…` is redacted as one unit.
    let bearer_scrubbed = BEARER_TOKEN_REGEX.replace_all(&kv_scrubbed, |caps: &regex::Captures| {
        format!("{} {}", &caps[1], redact_value(&caps[2]))
    });

    // 3. Standalone AWS access-key IDs.
    let aws_scrubbed = AWS_ACCESS_KEY_REGEX
        .replace_all(&bearer_scrubbed, |caps: &regex::Captures| {
            redact_value(&caps[1])
        });

    // 4. Standalone OpenAI-style `sk-` keys.
    OPENAI_KEY_REGEX
        .replace_all(&aws_scrubbed, |caps: &regex::Captures| {
            redact_value(&caps[1])
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_credentials_utf8() {
        // Regex requires at least 8 chars for the value
        // The [a-zA-Z0-9_\-\.]{8,} part of the regex does NOT match emoji
        // So we must use quotes to hit the "([^"]{8,})" part
        let input = "api_key: \"🦀🦀🦀🦀🦀🦀🦀🦀\"";
        let output = scrub_credentials(input);
        // Should preserve 4 crabs and then redact
        assert!(output.contains("🦀🦀🦀🦀*[REDACTED]"));
    }

    #[test]
    fn test_scrub_credentials_short_val() {
        let input = "api_key: 12345678";
        let output = scrub_credentials(input);
        assert!(output.contains("api_key: 1234*[REDACTED]"));
    }

    #[test]
    fn test_scrub_bare_aws_access_key() {
        // No `key: value` framing — a bare AWS access-key ID in shell/env output.
        let input = "export AWS: AKIAIOSFODNN7EXAMPLE is the id";
        let output = scrub_credentials(input);
        assert!(
            output.contains("AKIA*[REDACTED]"),
            "bare AWS key must be redacted, got: {output}"
        );
        assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn test_scrub_bare_openai_key() {
        let input = "the key is sk-abcdefghijklmnopqrstuvwxyz012345 ok";
        let output = scrub_credentials(input);
        assert!(
            output.contains("sk-a*[REDACTED]"),
            "bare sk- key must be redacted, got: {output}"
        );
        assert!(!output.contains("abcdefghijklmnop"));
    }

    #[test]
    fn test_scrub_bearer_token_space_separated() {
        // `Authorization: Bearer <token>` — the token is space-separated from the
        // scheme keyword, which the KV regex (needs `:`/`=`) never catches.
        let input = "Authorization: Bearer abcdef0123456789tokenvalue";
        let output = scrub_credentials(input);
        assert!(
            output.contains("Bearer abcd*[REDACTED]"),
            "bearer token must be redacted, got: {output}"
        );
        assert!(!output.contains("abcdef0123456789tokenvalue"));
    }

    #[test]
    fn test_scrub_idempotent() {
        let input = "api_key: sk-abcdefghijklmnopqrstuvwxyz012345";
        let once = scrub_credentials(input);
        let twice = scrub_credentials(&once);
        assert_eq!(
            once, twice,
            "scrubbing already-redacted text must be a no-op"
        );
    }
}
