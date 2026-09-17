use super::scrub_secrets;

#[test]
fn scrubs_bearer_token() {
    assert_eq!(
        scrub_secrets("Authorization: Bearer abc123xyz"),
        "Authorization: Bearer [REDACTED]"
    );
}

#[test]
fn scrubs_api_key_assignment() {
    assert_eq!(scrub_secrets("api_key=sk-abc123"), "api_key=[REDACTED]");
}

#[test]
fn scrubs_anthropic_key() {
    assert_eq!(
        scrub_secrets("key: sk-ant-api03-abcdefghijklmnop"),
        "key: [REDACTED]"
    );
}

#[test]
fn scrubs_bare_generic_sk_key() {
    assert_eq!(scrub_secrets("sk-abcdefghijklmnopqrstuvwx"), "[REDACTED]");
}

#[test]
fn scrubs_generic_sk_key_with_separators() {
    // A `_` or `-` mid-suffix must not leave a trailing fragment unredacted.
    assert_eq!(scrub_secrets("sk-abcdefghijklmnopqrst_uv"), "[REDACTED]");
    assert_eq!(scrub_secrets("sk-abcdefghij-klmnopqrst_uv"), "[REDACTED]");
}

#[test]
fn leaves_plain_diagnostics_intact() {
    let msg = "profile 42: derived rate clamp exceeded (max_iterations=8)";
    assert_eq!(scrub_secrets(msg), msg);
}
