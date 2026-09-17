use super::*;

#[test]
fn scrubs_bearer_token() {
    assert_eq!(
        scrub_secrets("Authorization: Bearer abc123xyz"),
        "Authorization: Bearer [REDACTED]"
    );
}

#[test]
fn scrubs_api_key() {
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
fn scrubs_openai_admin_key() {
    assert_eq!(
        scrub_secrets("key: sk-admin-abcdefghijkl"),
        "key: [REDACTED]"
    );
}

#[test]
fn scrubs_openai_proj_key() {
    assert_eq!(
        scrub_secrets("key: sk-proj-abcdefghijkl"),
        "key: [REDACTED]"
    );
}

#[test]
fn scrubs_generic_sk_key() {
    assert_eq!(scrub_secrets("sk-abcdefghijklmnopqrstuvwx"), "[REDACTED]");
}

#[test]
fn token_word_boundary_no_false_positive() {
    let input = "cancellation_token=abc123 next_page_token=xyz789";
    let result = scrub_secrets(input);
    assert_eq!(result, input, "should not scrub compound token fields");
}

#[test]
fn standalone_token_is_scrubbed() {
    assert_eq!(scrub_secrets("token=secret_value_here"), "token=[REDACTED]");
}
